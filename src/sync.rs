use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail, ensure};
use futures_util::{StreamExt, TryStreamExt, stream};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    adapters::{gated, modelsdev, public, vercel},
    http::{Fetcher, checked_next_url},
    model::{Catalog, Record, SCHEMA_VERSION, Snapshot},
    registry::{self, Registry, merge_catalog},
    store,
};

pub const PUBLIC_SOURCES: &[&str] = &[
    "modelsdev",
    "huggingface",
    "modelscope",
    "openrouter",
    "vercel",
    "official",
];
pub const AUTHENTICATED_SOURCES: &[&str] = &[
    "artificialanalysis",
    "openai",
    "anthropic",
    "gemini",
    "deepseek",
    "cohere",
    "fal",
    "replicate",
];

pub async fn run(root: &Path, requested: &[String], authenticated: bool) -> Result<()> {
    let sources = selected_sources(requested, authenticated)?;
    let cache = root.join(".cache");
    let fetcher = Fetcher::new(&cache)?;
    let mut registry = registry::load(root)?;
    registry.validate(None)?;
    let mut failures = Vec::new();
    for source in sources {
        eprintln!("{source}: fetching metadata");
        let result = async {
            if source == "official" {
                let catalog = crate::documents::sync(root, None).await?;
                let failures = catalog.documents.iter().filter(|d|d.error.is_some()).count();
                ensure!(failures == 0, "{failures} official pages failed; details saved in data/documents/official.yaml");
                return Ok(());
            }
            let incoming = fetch_catalog(&fetcher, root, source).await?;
            Registry {
                catalogs: vec![incoming.clone()],
                assertions: vec![],
            }
            .validate(Some(&cache))?;
            let index = registry
                .catalogs
                .iter()
                .position(|catalog| catalog.source == source);
            let old = index.map(|index| registry.catalogs[index].clone());
            let completed_at = OffsetDateTime::now_utc().format(&Rfc3339)?;
            let merged = merge_catalog(old, incoming, &completed_at)?;
            Registry {
                catalogs: vec![merged.clone()],
                assertions: vec![],
            }
            .validate(None)?;
            let changed = store::write_yaml(
                &root.join("data/catalogs").join(format!("{source}.yaml")),
                &merged,
            )?;
            eprintln!(
                "{source}: {} records, {}",
                merged.records.len(),
                if changed { "updated" } else { "unchanged" }
            );
            if let Some(index) = index {
                registry.catalogs[index] = merged;
            } else {
                registry.catalogs.push(merged);
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            eprintln!("{source}: {error:#}");
            failures.push(source);
        }
    }
    ensure!(
        failures.is_empty(),
        "sync failed for: {}",
        failures.join(", ")
    );
    Ok(())
}

fn selected_sources(requested: &[String], authenticated: bool) -> Result<Vec<&str>> {
    ensure!(
        !authenticated || !requested.is_empty(),
        "--authenticated requires explicit --source"
    );
    let sources = if requested.is_empty() {
        PUBLIC_SOURCES.to_vec()
    } else {
        requested.iter().map(String::as_str).collect()
    };
    let mut seen = BTreeSet::new();
    for source in &sources {
        ensure!(seen.insert(*source), "duplicate source: {source}");
        ensure!(
            PUBLIC_SOURCES.contains(source) || AUTHENTICATED_SOURCES.contains(source),
            "unknown source: {source}"
        );
        ensure!(
            authenticated || !AUTHENTICATED_SOURCES.contains(source),
            "{source} requires explicit --authenticated; no credentials were read and no requests were sent"
        );
    }
    Ok(sources)
}

async fn fetch_catalog(fetcher: &Fetcher, root: &Path, source: &str) -> Result<Catalog> {
    let mut catalog = Catalog {
        schema_version: SCHEMA_VERSION,
        source: source.into(),
        last_sync: None,
        snapshots: vec![],
        records: vec![],
    };
    match source {
        "vercel" => {
            let (snapshot, value) = fetcher
                .get_json(
                    source,
                    "https://ai-gateway.vercel.sh/v1/models",
                    HeaderMap::new(),
                )
                .await?;
            catalog.records = vercel::models(&value, &snapshot)?;
            catalog.snapshots.push(snapshot);
        }
        "modelsdev" => {
            for (url, normalize) in [
                (
                    "https://models.dev/models.json?type=all",
                    modelsdev::models as Normalizer,
                ),
                (
                    "https://models.dev/catalog.json?type=all",
                    modelsdev::services as Normalizer,
                ),
            ] {
                let (snapshot, value) = fetcher.get_json(source, url, HeaderMap::new()).await?;
                catalog.records.extend(normalize(&value, &snapshot)?);
                catalog.snapshots.push(snapshot);
            }
        }
        "huggingface" | "modelscope" => {
            let ids = read_seeds(&root.join("config").join(format!("{source}-models.txt")))?;
            let base = if source == "huggingface" {
                "https://huggingface.co/api/models/"
            } else {
                "https://modelscope.cn/api/v1/models/"
            };
            let batches: Vec<_> = stream::iter(ids)
                .map(|id| async move {
                    let (snapshot, value) = fetcher
                        .get_json(source, &format!("{base}{id}"), HeaderMap::new())
                        .await?;
                    let records = if source == "huggingface" {
                        public::huggingface(&value, &snapshot)?
                    } else {
                        public::modelscope(&value, &snapshot)?
                    };
                    ensure!(
                        records.len() == 1 && records[0].upstream_id == id,
                        "{source} response identity differs from requested {id}"
                    );
                    Ok::<_, anyhow::Error>((snapshot, records))
                })
                .buffer_unordered(4)
                .try_collect()
                .await?;
            for (snapshot, records) in batches {
                catalog.snapshots.push(snapshot);
                catalog.records.extend(records);
            }
        }
        _ => {
            let mut headers = HeaderMap::new();
            let mut url = if source == "openrouter" {
                "https://openrouter.ai/api/v1/models?output_modalities=all&limit=1000".to_owned()
            } else {
                let spec = gated::request_spec(source).context("missing request specification")?;
                let secret = std::env::var(spec.credential_env).map_err(|_| {
                    anyhow::anyhow!(
                        "set {} to a valid UTF-8 credential to sync {source}",
                        spec.credential_env
                    )
                })?;
                ensure!(
                    !secret.trim().is_empty(),
                    "{} must not be empty",
                    spec.credential_env
                );
                let mut credential = HeaderValue::from_str(&format!("{}{secret}", spec.prefix))
                    .context("credential cannot be used as an HTTP header")?;
                credential.set_sensitive(true);
                headers.insert(spec.header, credential);
                for (name, value) in spec.extra_headers {
                    headers.insert(name, HeaderValue::from_static(value));
                }
                spec.url.to_owned()
            };
            let mut visited = BTreeSet::new();
            loop {
                ensure!(
                    visited.len() < 1000 && visited.insert(url.clone()),
                    "{source}: pagination cycle or page limit exceeded"
                );
                let (snapshot, value) = fetcher.get_json(source, &url, headers.clone()).await?;
                let records = if source == "openrouter" {
                    public::openrouter(&value, &snapshot)?
                } else {
                    gated::normalize(source, &value, &snapshot)?
                };
                let next = if source == "openrouter" {
                    openrouter_next(&value)?
                } else {
                    gated::next_page(source, &value, &url)?
                };
                catalog.records.extend(records);
                catalog.snapshots.push(snapshot);
                match next {
                    Some(next) => url = checked_next_url(&url, &next)?,
                    None => break,
                }
            }
        }
    }
    catalog.snapshots.sort_by(|a, b| a.id.cmp(&b.id));
    catalog.snapshots.dedup_by(|a, b| a.id == b.id);
    catalog.records.sort_by(|a, b| a.id.cmp(&b.id));
    ensure!(
        !catalog.records.is_empty(),
        "{source}: refusing empty catalog"
    );
    Ok(catalog)
}

type Normalizer = fn(&Value, &Snapshot) -> Result<Vec<Record>>;

fn read_seeds(path: &Path) -> Result<Vec<String>> {
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut ids = BTreeSet::new();
    for line in contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let segments: Vec<_> = line.split('/').collect();
        ensure!(
            segments.len() == 2
                && segments.iter().all(|part| !part.is_empty()
                    && *part != "."
                    && *part != ".."
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))),
            "invalid seed id: {line}"
        );
        ensure!(ids.insert(line.to_owned()), "duplicate seed id: {line}");
    }
    ensure!(!ids.is_empty(), "seed file is empty: {}", path.display());
    Ok(ids.into_iter().collect())
}

fn openrouter_next(value: &Value) -> Result<Option<String>> {
    match value.pointer("/links/next") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(next)) if !next.is_empty() => Ok(Some(next.clone())),
        _ => bail!("OpenRouter links.next must be a URL or null"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_requires_explicit_source_and_opt_in_before_network() {
        assert_eq!(selected_sources(&[], false).unwrap(), PUBLIC_SOURCES);
        assert!(selected_sources(&[], true).is_err());
        for source in AUTHENTICATED_SOURCES {
            let requested = vec![(*source).into()];
            assert!(selected_sources(&requested, false).is_err());
            assert_eq!(selected_sources(&requested, true).unwrap(), [*source]);
        }
        assert!(selected_sources(&["unknown".into()], true).is_err());
        assert!(selected_sources(&["openrouter".into(), "openrouter".into()], false).is_err());
    }

    #[test]
    fn seeds_reject_paths_and_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("models.txt");
        for text in ["../model", "a/../../b", "a/b?x=1", "a/b\na/b", "# empty"] {
            fs::write(&file, text).unwrap();
            assert!(read_seeds(&file).is_err(), "{text}");
        }
        fs::write(&file, "# comment\n org/Model-1.0 \na/b\n").unwrap();
        assert_eq!(read_seeds(&file).unwrap(), ["a/b", "org/Model-1.0"]);
        assert!(openrouter_next(&serde_json::json!({"links":{"next":true}})).is_err());
    }
}

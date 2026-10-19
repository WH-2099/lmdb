use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail, ensure};
use futures_util::{StreamExt, stream};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    adapters::{gated, modelsdev, public, vercel},
    http::{Fetcher, checked_next_url},
    model::{
        Catalog, Record, SCHEMA_VERSION, Snapshot, SyncFailure, SyncManifest, SyncScope, SyncStatus,
    },
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
    let plans = sources
        .into_iter()
        .map(|source| Ok((source, prepare_source(root, source)?)))
        .collect::<Result<Vec<_>>>()?;
    let cache = root.join(".cache");
    let fetcher = Fetcher::new(&cache)?;
    let mut registry = registry::load(root)?;
    registry.validate(None)?;
    let mut failures = Vec::new();
    for (source, plan) in plans {
        eprintln!("{source}: fetching metadata");
        let result = async {
            if source == "official" {
                let catalog = crate::documents::sync(root, None).await?;
                let failures = catalog.documents.iter().filter(|d|d.error.is_some()).count();
                ensure!(failures == 0, "{failures} official pages failed; details saved in data/documents/official.yaml");
                return Ok(());
            }
            let incoming = fetch_catalog(&fetcher, source, plan).await?;
            save_catalog(root, &mut registry, incoming)?;
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

fn save_catalog(root: &Path, registry: &mut Registry, incoming: Catalog) -> Result<()> {
    Registry {
        catalogs: vec![incoming.clone()],
        assertions: vec![],
    }
    .validate(Some(&root.join(".cache")))?;
    let source = incoming.source.clone();
    let index = registry
        .catalogs
        .iter()
        .position(|catalog| catalog.source == source);
    let old = index.map(|index| registry.catalogs[index].clone());
    let completed_at = incoming
        .last_sync
        .as_ref()
        .context("missing attempt manifest")?
        .completed_at
        .clone();
    let merged = merge_catalog(old, incoming, &completed_at)?;
    Registry {
        catalogs: vec![merged.clone()],
        assertions: vec![],
    }
    .validate(None)?;
    let success = merged
        .last_sync
        .as_ref()
        .is_some_and(|sync| sync.status == Some(SyncStatus::Complete));
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
    ensure!(
        success,
        "collection incomplete; successful metadata and failure details saved in data/catalogs/{source}.yaml"
    );
    Ok(())
}

fn empty_catalog(source: &str) -> Catalog {
    Catalog {
        schema_version: SCHEMA_VERSION,
        source: source.into(),
        last_sync: None,
        snapshots: vec![],
        records: vec![],
    }
}

enum FetchPlan {
    Selected {
        ids: Vec<String>,
        base: &'static str,
    },
    Pages {
        url: String,
        headers: HeaderMap,
    },
    Vercel,
    ModelsDev,
    Official,
}

fn prepare_source(root: &Path, source: &str) -> Result<FetchPlan> {
    Ok(match source {
        "huggingface" | "modelscope" => FetchPlan::Selected {
            ids: read_seeds(&root.join("config").join(format!("{source}-models.txt")))?,
            base: if source == "huggingface" {
                "https://huggingface.co/api/models/"
            } else {
                "https://modelscope.cn/api/v1/models/"
            },
        },
        "vercel" => FetchPlan::Vercel,
        "modelsdev" => FetchPlan::ModelsDev,
        "official" => FetchPlan::Official,
        _ => {
            let (url, headers) = catalog_request(source)?;
            FetchPlan::Pages { url, headers }
        }
    })
}

async fn fetch_catalog(fetcher: &Fetcher, source: &str, plan: FetchPlan) -> Result<Catalog> {
    let mut catalog = empty_catalog(source);
    let mut scope = Some(SyncScope::FullCatalog);
    let mut errors = Vec::new();
    let result = async {
        match plan {
            FetchPlan::Selected { ids, base } => {
                scope = Some(SyncScope::SelectedModels {
                    upstream_ids: ids.clone(),
                });
                errors = fetch_selected(fetcher, &mut catalog, &ids, base).await;
            }
            FetchPlan::Vercel => {
                let (snapshot, value) = fetcher
                    .get_json(
                        source,
                        "https://ai-gateway.vercel.sh/v1/models",
                        HeaderMap::new(),
                    )
                    .await?;
                catalog.snapshots.push(snapshot.clone());
                catalog.records = vercel::models(&value, &snapshot)?;
            }
            FetchPlan::ModelsDev => {
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
                    catalog.snapshots.push(snapshot.clone());
                    catalog.records.extend(normalize(&value, &snapshot)?);
                }
            }
            FetchPlan::Pages { url, headers } => {
                fetch_pages(fetcher, &mut catalog, url, headers).await?;
            }
            FetchPlan::Official => bail!("official pages require the document collector"),
        }
        for record in &catalog.records {
            crate::domain::project(record)?;
        }
        ensure!(
            !catalog.records.is_empty() || !errors.is_empty(),
            "{source}: refusing empty catalog"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if let Err(error) = result {
        // A directory is meaningful only when all pages have been collected.
        catalog.records.clear();
        errors = vec![SyncFailure {
            upstream_id: None,
            message: failure_message(&error),
        }];
    }
    finish_attempt(&mut catalog, scope, errors)?;
    Ok(catalog)
}

async fn fetch_selected(
    fetcher: &Fetcher,
    catalog: &mut Catalog,
    ids: &[String],
    base: &str,
) -> Vec<SyncFailure> {
    let source = catalog.source.as_str();
    let batches: Vec<_> = stream::iter(ids)
        .map(|id| async move {
            let fetched = fetcher
                .get_json(source, &format!("{base}{id}"), HeaderMap::new())
                .await;
            let (snapshot, result) = match fetched {
                Ok((snapshot, value)) => {
                    let result = (|| {
                        let records = if source == "huggingface" {
                            public::huggingface(&value, &snapshot)?
                        } else {
                            public::modelscope(&value, &snapshot)?
                        };
                        ensure!(
                            records.len() == 1 && records[0].upstream_id == *id,
                            "response identity differs from requested model"
                        );
                        for record in &records {
                            crate::domain::project(record)?;
                        }
                        Ok(records)
                    })();
                    (Some(snapshot), result)
                }
                Err(error) => (None, Err(error)),
            };
            (id, snapshot, result)
        })
        .buffer_unordered(4)
        .collect()
        .await;
    let mut errors = Vec::new();
    for (id, snapshot, result) in batches {
        if let Some(snapshot) = snapshot {
            catalog.snapshots.push(snapshot);
        }
        match result {
            Ok(records) => catalog.records.extend(records),
            Err(error) => errors.push(SyncFailure {
                upstream_id: Some(id.clone()),
                message: failure_message(&error),
            }),
        }
    }
    errors.sort_by(|a, b| a.upstream_id.cmp(&b.upstream_id));
    errors
}

fn catalog_request(source: &str) -> Result<(String, HeaderMap)> {
    if source == "openrouter" {
        return Ok((
            "https://openrouter.ai/api/v1/models?output_modalities=all&limit=1000".into(),
            HeaderMap::new(),
        ));
    }
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
        .with_context(|| format!("{} cannot be used as an HTTP header", spec.credential_env))?;
    credential.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert(spec.header, credential);
    for (name, value) in spec.extra_headers {
        headers.insert(name, HeaderValue::from_static(value));
    }
    Ok((spec.url.into(), headers))
}

async fn fetch_pages(
    fetcher: &Fetcher,
    catalog: &mut Catalog,
    mut url: String,
    headers: HeaderMap,
) -> Result<()> {
    let source = catalog.source.as_str();
    let mut visited = BTreeSet::new();
    let mut records = Vec::new();
    loop {
        ensure!(
            visited.len() < 1000 && visited.insert(url.clone()),
            "{source}: pagination cycle or page limit exceeded"
        );
        let (snapshot, value) = fetcher.get_json(source, &url, headers.clone()).await?;
        catalog.snapshots.push(snapshot.clone());
        let page = if source == "openrouter" {
            public::openrouter(&value, &snapshot)?
        } else {
            gated::normalize(source, &value, &snapshot)?
        };
        let next = if source == "openrouter" {
            openrouter_next(&value)?
        } else {
            gated::next_page(source, &value, &url)?
        };
        records.extend(page);
        match next {
            Some(next) => url = checked_next_url(&url, &next)?,
            None => break,
        }
    }
    ensure!(!records.is_empty(), "{source}: refusing empty catalog");
    let mut ids = BTreeSet::new();
    for record in &records {
        crate::domain::project(record)?;
        ensure!(
            ids.insert(&record.id),
            "duplicate record across catalog pages"
        );
    }
    catalog.records = records;
    Ok(())
}

fn finish_attempt(
    catalog: &mut Catalog,
    scope: Option<SyncScope>,
    mut errors: Vec<SyncFailure>,
) -> Result<()> {
    catalog.snapshots.sort_by(|a, b| a.id.cmp(&b.id));
    catalog.snapshots.dedup_by(|a, b| a.id == b.id);
    catalog.records.sort_by(|a, b| a.id.cmp(&b.id));
    if let Err(error) = validate_incoming(catalog, scope.as_ref(), &mut errors) {
        catalog.records.clear();
        let message = failure_message(&error);
        errors = match &scope {
            Some(SyncScope::SelectedModels { upstream_ids }) => upstream_ids
                .iter()
                .map(|id| SyncFailure {
                    upstream_id: Some(id.clone()),
                    message: message.clone(),
                })
                .collect(),
            _ => vec![SyncFailure {
                upstream_id: None,
                message,
            }],
        };
    }
    errors.sort_by(|a, b| a.upstream_id.cmp(&b.upstream_id));
    let status = if errors.is_empty() {
        SyncStatus::Complete
    } else if catalog.records.is_empty() {
        SyncStatus::Failed
    } else {
        SyncStatus::Partial
    };
    let completed_at = OffsetDateTime::now_utc().format(&Rfc3339)?;
    catalog.last_sync = Some(SyncManifest {
        last_successful_at: (status == SyncStatus::Complete).then(|| completed_at.clone()),
        completed_at,
        returned_ids: catalog
            .records
            .iter()
            .map(|record| record.id.clone())
            .collect(),
        missing_ids: Vec::new(),
        snapshot_ids: catalog
            .snapshots
            .iter()
            .map(|snapshot| snapshot.id.clone())
            .collect(),
        scope,
        status: Some(status),
        errors,
    });
    Ok(())
}

fn validate_incoming(
    catalog: &mut Catalog,
    scope: Option<&SyncScope>,
    errors: &mut Vec<SyncFailure>,
) -> Result<()> {
    if matches!(scope, Some(SyncScope::SelectedModels { .. })) {
        for record in std::mem::take(&mut catalog.records) {
            let batch = Catalog {
                schema_version: catalog.schema_version,
                source: catalog.source.clone(),
                last_sync: None,
                snapshots: catalog
                    .snapshots
                    .iter()
                    .filter(|snapshot| snapshot.id == record.origin.snapshot)
                    .cloned()
                    .collect(),
                records: vec![record.clone()],
            };
            match (Registry {
                catalogs: vec![batch],
                assertions: vec![],
            })
            .validate(None)
            {
                Ok(()) => catalog.records.push(record),
                Err(error) => errors.push(SyncFailure {
                    upstream_id: Some(record.upstream_id),
                    message: failure_message(&error),
                }),
            }
        }
    }
    ensure!(
        !catalog.records.is_empty() || !errors.is_empty(),
        "refusing empty catalog"
    );
    Registry {
        catalogs: vec![catalog.clone()],
        assertions: vec![],
    }
    .validate(None)
}

fn failure_message(error: &anyhow::Error) -> String {
    // URLs can contain provider pagination tokens; persist a bounded diagnostic without them.
    error
        .to_string()
        .split_whitespace()
        .map(|word| if word.contains("://") { "[URL]" } else { word })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(512)
        .collect()
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

    fn server(routes: Vec<(&str, u16, String)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::Duration,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let routes: std::collections::BTreeMap<_, _> = routes
            .into_iter()
            .map(|(path, status, body)| (path.to_owned(), (status, body)))
            .collect();
        let worker = std::thread::spawn(move || {
            (0..routes.len()).map(|_| {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap();
                let (status, body) = routes.get(path).unwrap();
                write!(socket, "HTTP/1.1 {status} Fixture\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
                path.to_owned()
            }).collect()
        });
        (url, worker)
    }

    fn persisted_fixture_urls(catalog: &mut Catalog) {
        for snapshot in &mut catalog.snapshots {
            // The transport fixture uses HTTP locally; registry evidence requires an HTTPS origin.
            snapshot.url = snapshot.url.replacen("http://", "https://", 1);
        }
    }

    #[tokio::test]
    async fn selected_requests_save_successes_and_failures_then_recover() -> Result<()> {
        use crate::model::ObservationStatus;
        for source in ["huggingface", "modelscope"] {
            let root = tempfile::tempdir()?;
            let fetcher = Fetcher::new(&root.path().join(".cache"))?;
            let body = |name: &str, revision: &str| {
                if source == "huggingface" {
                    serde_json::json!({"id":format!("lab/{name}"), "sha":revision}).to_string()
                } else {
                    serde_json::json!({"Code":200,"Success":true,"Data":{"Path":"lab","Name":name,"Revision":revision}}).to_string()
                }
            };
            let mut registry = Registry {
                catalogs: vec![],
                assertions: vec![],
            };
            let ids = vec!["lab/a".into(), "lab/b".into(), "lab/c".into()];
            let (base, worker) = server(vec![
                ("/lab/a", 200, body("a", "old")),
                ("/lab/b", 200, body("b", "old")),
                ("/lab/c", 200, body("c", "old")),
            ]);
            let mut first = empty_catalog(source);
            let errors = fetch_selected(&fetcher, &mut first, &ids, &base).await;
            assert!(errors.is_empty());
            assert_eq!(worker.join().unwrap().len(), 3);
            persisted_fixture_urls(&mut first);
            finish_attempt(
                &mut first,
                Some(SyncScope::SelectedModels { upstream_ids: ids }),
                errors,
            )?;

            save_catalog(root.path(), &mut registry, first)?;
            let old = registry.catalogs[0].clone();

            let ids = vec!["lab/a".into(), "lab/b".into(), "lab/new".into()];
            let (base, worker) = server(vec![
                ("/lab/a", 200, body("a", "new")),
                ("/lab/b", 404, "{}".into()),
                ("/lab/new", 200, body("wrong", "new")),
            ]);
            let mut partial = empty_catalog(source);
            let errors = fetch_selected(&fetcher, &mut partial, &ids, &base).await;
            assert_eq!(worker.join().unwrap().len(), 3);
            assert_eq!(errors.len(), 2);
            assert_eq!(partial.snapshots.len(), 2);
            persisted_fixture_urls(&mut partial);
            finish_attempt(
                &mut partial,
                Some(SyncScope::SelectedModels { upstream_ids: ids }),
                errors,
            )?;

            assert!(save_catalog(root.path(), &mut registry, partial).is_err());
            let saved: Catalog =
                store::read_yaml(&root.path().join(format!("data/catalogs/{source}.yaml")))?;
            assert_eq!(saved, registry.catalogs[0]);
            let sync = saved.last_sync.as_ref().unwrap();
            assert_eq!(sync.status, Some(SyncStatus::Partial));
            assert_eq!(
                sync.last_successful_at,
                old.last_sync.as_ref().unwrap().last_successful_at
            );
            assert!(sync.missing_ids.is_empty());
            for (index, expected) in [
                ObservationStatus::Observed,
                ObservationStatus::Failed,
                ObservationStatus::NotRequested,
            ]
            .into_iter()
            .enumerate()
            {
                assert_eq!(saved.observation(&saved.records[index]), expected);
                if index > 0 {
                    assert_eq!(saved.records[index], old.records[index]);
                }
            }
            assert_eq!(saved.records[0].facts["revision"].value, "new");

            let ids = vec!["lab/a".into(), "lab/b".into()];
            let (base, worker) = server(vec![
                ("/lab/a", 404, "{}".into()),
                ("/lab/b", 404, "{}".into()),
            ]);
            let mut failed = empty_catalog(source);
            let errors = fetch_selected(&fetcher, &mut failed, &ids, &base).await;
            worker.join().unwrap();
            persisted_fixture_urls(&mut failed);
            finish_attempt(
                &mut failed,
                Some(SyncScope::SelectedModels {
                    upstream_ids: ids.clone(),
                }),
                errors,
            )?;
            assert!(save_catalog(root.path(), &mut registry, failed).is_err());
            assert_eq!(registry.catalogs[0].records, saved.records);
            assert_eq!(
                registry.catalogs[0].last_sync.as_ref().unwrap().status,
                Some(SyncStatus::Failed)
            );

            let (base, worker) = server(vec![
                ("/lab/a", 200, body("a", "new")),
                ("/lab/b", 200, body("b", "restored")),
            ]);
            let mut restored = empty_catalog(source);
            let errors = fetch_selected(&fetcher, &mut restored, &ids, &base).await;
            worker.join().unwrap();
            persisted_fixture_urls(&mut restored);
            finish_attempt(
                &mut restored,
                Some(SyncScope::SelectedModels { upstream_ids: ids }),
                errors,
            )?;

            save_catalog(root.path(), &mut registry, restored)?;
            let restored = &registry.catalogs[0];
            assert!(restored.last_sync.as_ref().unwrap().errors.is_empty());
            assert_eq!(
                restored.observation(&restored.records[1]),
                ObservationStatus::Observed
            );
            assert_eq!(
                restored.observation(&restored.records[2]),
                ObservationStatus::NotRequested
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn incomplete_pagination_preserves_catalog_and_does_not_infer_absence() -> Result<()> {
        use crate::model::ObservationStatus;
        let root = tempfile::tempdir()?;
        let fetcher = Fetcher::new(&root.path().join(".cache"))?;
        let mut registry = Registry {
            catalogs: vec![],
            assertions: vec![],
        };
        let (base, worker) = server(vec![(
            "/",
            200,
            r#"{"data":[{"id":"lab/a","context_length":42},{"id":"lab/b"}]}"#.into(),
        )]);
        let mut first = empty_catalog("openrouter");
        fetch_pages(&fetcher, &mut first, base, HeaderMap::new()).await?;
        worker.join().unwrap();
        persisted_fixture_urls(&mut first);
        finish_attempt(&mut first, Some(SyncScope::FullCatalog), vec![])?;

        save_catalog(root.path(), &mut registry, first)?;
        let old = registry.catalogs[0].clone();
        let (base, worker) = server(vec![
            ("/",200,r#"{"data":[{"id":"lab/a","context_length":99}],"links":{"next":"/page2?token=private"}}"#.into()),
            ("/page2?token=private",403,"{}".into()),
        ]);
        let mut failed = empty_catalog("openrouter");
        let error = fetch_pages(&fetcher, &mut failed, base, HeaderMap::new())
            .await
            .unwrap_err();
        worker.join().unwrap();
        assert!(failed.records.is_empty());
        assert_eq!(failed.snapshots.len(), 1);
        let message = failure_message(&error);
        assert!(message.contains("403"));
        assert!(!message.contains("private"));
        persisted_fixture_urls(&mut failed);
        finish_attempt(
            &mut failed,
            Some(SyncScope::FullCatalog),
            vec![SyncFailure {
                upstream_id: None,
                message,
            }],
        )?;

        assert!(save_catalog(root.path(), &mut registry, failed).is_err());
        let failed = &registry.catalogs[0];
        assert_eq!(failed.records, old.records);
        let sync = failed.last_sync.as_ref().unwrap();
        assert!(sync.returned_ids.is_empty() && sync.missing_ids.is_empty());
        assert_eq!(
            sync.last_successful_at,
            old.last_sync.as_ref().unwrap().last_successful_at
        );
        assert!(
            failed
                .records
                .iter()
                .all(|record| failed.observation(record) == ObservationStatus::Failed)
        );

        let (base, worker) = server(vec![(
            "/",
            200,
            r#"{"data":[{"id":"lab/a","context_length":99}]}"#.into(),
        )]);
        let mut restored = empty_catalog("openrouter");
        fetch_pages(&fetcher, &mut restored, base, HeaderMap::new()).await?;
        worker.join().unwrap();
        persisted_fixture_urls(&mut restored);
        finish_attempt(&mut restored, Some(SyncScope::FullCatalog), vec![])?;

        save_catalog(root.path(), &mut registry, restored)?;
        let restored = &registry.catalogs[0];
        assert_eq!(
            restored.observation(&restored.records[0]),
            ObservationStatus::Observed
        );
        assert_eq!(
            restored.observation(&restored.records[1]),
            ObservationStatus::Missing
        );
        Ok(())
    }

    #[tokio::test]
    async fn invalid_seed_configuration_is_rejected_before_loading_or_writing_catalogs()
    -> Result<()> {
        for source in ["huggingface", "modelscope"] {
            let root = tempfile::tempdir()?;
            let config = root.path().join(format!("config/{source}-models.txt"));
            let existing = root.path().join("data/catalogs/invalid.yaml");
            fs::create_dir_all(existing.parent().unwrap())?;
            fs::write(&existing, "schema_version: [")?;
            for contents in [None, Some("# empty"), Some("bad/id/extra")] {
                if let Some(contents) = contents {
                    fs::create_dir_all(config.parent().unwrap())?;
                    fs::write(&config, contents)?;
                }
                let error = run(root.path(), &[source.into()], false).await.unwrap_err();
                let message = format!("{error:#}");
                assert!(
                    message.contains("seed") || message.contains("config/"),
                    "{message}"
                );
                assert!(!message.contains("invalid.yaml"), "{message}");
                assert!(!root.path().join(".cache").exists());
                assert!(
                    !root
                        .path()
                        .join(format!("data/catalogs/{source}.yaml"))
                        .exists()
                );
                assert_eq!(fs::read_to_string(&existing)?, "schema_version: [");
            }
        }
        Ok(())
    }

    fn fixture_catalog(
        root: &Path,
        source: &str,
        path: &str,
        value: Value,
        normalize: Normalizer,
    ) -> Result<Catalog> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(&value)?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let snapshot = Snapshot {
            schema_version: SCHEMA_VERSION,
            id: format!("{source}:{sha256}"),
            source: source.into(),
            url: format!("https://fixture.test{path}"),
            retrieved_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
            sha256: sha256.clone(),
            upstream_revision: None,
        };
        store::write_bytes(
            &root.join(".cache/snapshots").join(format!("{sha256}.json")),
            &bytes,
        )?;
        let records = normalize(&value, &snapshot)?;
        Ok(Catalog {
            snapshots: vec![snapshot],
            records,
            ..empty_catalog(source)
        })
    }

    #[test]
    fn malformed_complete_catalogs_record_failure_and_keep_previous_evidence() -> Result<()> {
        use crate::model::ObservationStatus;
        use serde_json::json;
        let cases = [
            (
                "vercel",
                "/v1/models",
                json!({"data":[{"id":"a"}]}),
                json!({"data":[{"id":"a"},{"id":"a"}]}),
                vercel::models as Normalizer,
                "duplicate record",
            ),
            (
                "modelsdev",
                "/catalog.json",
                json!({"providers":{"host":{"id":"host","models":{"a":{"id":"a"}}}}}),
                json!({"providers":{"host":{"id":"host","models":{"a":{"id":"a","canonical_model_id":"missing"}}}}}),
                modelsdev::services as Normalizer,
                "model must reference",
            ),
        ];
        for (source, path, good, bad, normalize, diagnostic) in cases {
            let root = tempfile::tempdir()?;
            let mut registry = Registry {
                catalogs: vec![],
                assertions: vec![],
            };
            let mut first = fixture_catalog(root.path(), source, path, good, normalize)?;
            finish_attempt(&mut first, Some(SyncScope::FullCatalog), vec![])?;
            save_catalog(root.path(), &mut registry, first)?;
            let old = registry.catalogs[0].clone();
            let mut malformed = fixture_catalog(root.path(), source, path, bad, normalize)?;
            let rejected_snapshot = malformed.snapshots[0].id.clone();
            finish_attempt(&mut malformed, Some(SyncScope::FullCatalog), vec![])?;
            assert_eq!(
                malformed.last_sync.as_ref().unwrap().status,
                Some(SyncStatus::Failed)
            );
            assert!(save_catalog(root.path(), &mut registry, malformed).is_err());
            let saved: Catalog =
                store::read_yaml(&root.path().join(format!("data/catalogs/{source}.yaml")))?;
            assert_eq!(saved.records, old.records);
            let attempt = saved.last_sync.as_ref().unwrap();
            assert_eq!(attempt.status, Some(SyncStatus::Failed));
            assert_eq!(
                attempt.last_successful_at,
                old.last_sync.as_ref().unwrap().last_successful_at
            );
            assert!(attempt.returned_ids.is_empty() && attempt.missing_ids.is_empty());
            assert!(
                attempt.errors[0].message.contains(diagnostic),
                "{}",
                attempt.errors[0].message
            );
            assert_eq!(attempt.snapshot_ids, [rejected_snapshot]);
            assert_eq!(
                saved.observation(&saved.records[0]),
                ObservationStatus::Failed
            );
        }
        Ok(())
    }

    #[test]
    fn selected_structure_errors_are_attributed_without_discarding_valid_seeds() -> Result<()> {
        use crate::model::ObservationStatus;
        use serde_json::json;
        let root = tempfile::tempdir()?;
        let scope = Some(SyncScope::SelectedModels {
            upstream_ids: vec!["lab/a".into(), "lab/b".into()],
        });
        let batch = |revision: &str| -> Result<Catalog> {
            let mut result = empty_catalog("huggingface");
            for id in ["lab/a", "lab/b"] {
                let catalog = fixture_catalog(
                    root.path(),
                    "huggingface",
                    &format!("/api/models/{id}"),
                    json!({"id":id,"sha":revision}),
                    public::huggingface,
                )?;
                result.records.extend(catalog.records);
                result.snapshots.extend(catalog.snapshots);
            }
            Ok(result)
        };
        let mut registry = Registry {
            catalogs: vec![],
            assertions: vec![],
        };
        let mut first = batch("old")?;
        finish_attempt(&mut first, scope.clone(), vec![])?;
        save_catalog(root.path(), &mut registry, first)?;
        let old = registry.catalogs[0].clone();
        let mut partial = batch("new")?;
        partial.records[1].name = " ".into();
        finish_attempt(&mut partial, scope, vec![])?;
        assert_eq!(partial.records.len(), 1);
        let attempt = partial.last_sync.as_ref().unwrap();
        assert_eq!(attempt.status, Some(SyncStatus::Partial));
        assert_eq!(attempt.errors[0].upstream_id.as_deref(), Some("lab/b"));
        assert!(save_catalog(root.path(), &mut registry, partial).is_err());
        let saved = &registry.catalogs[0];
        assert_eq!(saved.records[0].facts["revision"].value, "new");
        assert_eq!(saved.records[1], old.records[1]);
        assert_eq!(
            saved.observation(&saved.records[0]),
            ObservationStatus::Observed
        );
        assert_eq!(
            saved.observation(&saved.records[1]),
            ObservationStatus::Failed
        );
        Ok(())
    }
}

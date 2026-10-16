mod asia;
pub mod compare;
mod content;
mod details_asia;
mod details_west;
pub mod model;
pub mod page;
pub mod resolve;
mod west;
pub use model::*;

use crate::{
    http::Fetcher,
    model::RecordKind,
    registry::{self, Registry},
    store,
};
use anyhow::{Context, Result, ensure};
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub const EXTRACTOR_VERSION: u32 = 2;
const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;

pub fn is_official_url(vendor: &str, raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let (domains, authors): (&[&str], &[&str]) = match vendor {
        "openai" => (
            &[
                "openai.com",
                "www.openai.com",
                "developers.openai.com",
                "platform.openai.com",
            ],
            &["openai"],
        ),
        "anthropic" => (
            &[
                "anthropic.com",
                "www.anthropic.com",
                "docs.anthropic.com",
                "platform.claude.com",
                "assets.anthropic.com",
            ],
            &["anthropics"],
        ),
        "google" => (
            &[
                "blog.google",
                "ai.google.dev",
                "cloud.google.com",
                "docs.cloud.google.com",
                "deepmind.google",
            ],
            &["google", "google-deepmind"],
        ),
        "meta" => (
            &["ai.meta.com", "www.llama.com", "llama.com", "dev.meta.ai"],
            &["meta-llama"],
        ),
        "xai" => (&["x.ai", "docs.x.ai", "data.x.ai"], &["xai-org"]),
        "amazon" => (
            &["aws.amazon.com", "docs.aws.amazon.com", "nova.amazon.com"],
            &["amazon", "aws"],
        ),
        "deepseek" => (
            &["api-docs.deepseek.com", "www.deepseek.com", "deepseek.com"],
            &["deepseek-ai"],
        ),
        "qwen" => (&["qwenlm.github.io", "qwen.ai"], &["qwen", "qwenlm"]),
        "nvidia" => (
            &[
                "blogs.nvidia.com",
                "build.nvidia.com",
                "www.nvidia.com",
                "docs.nvidia.com",
                "research.nvidia.com",
            ],
            &["nvidia", "nvidia-nemo"],
        ),
        "mistral" => (&["mistral.ai", "docs.mistral.ai"], &["mistralai"]),
        "cohere" => (
            &["cohere.com", "docs.cohere.com"],
            &["cohereforai", "cohere-ai"],
        ),
        "microsoft" => (
            &[
                "techcommunity.microsoft.com",
                "www.microsoft.com",
                "learn.microsoft.com",
                "azure.microsoft.com",
            ],
            &["microsoft"],
        ),
        _ => return false,
    };
    if domains.contains(&host) {
        return true;
    }
    if ["huggingface.co", "github.com", "raw.githubusercontent.com"].contains(&host) {
        let mut segments = url.path_segments().into_iter().flatten();
        let first = segments.next();
        let author = if host == "huggingface.co"
            && matches!(first, Some("collections" | "datasets" | "spaces"))
        {
            segments.next()
        } else {
            first
        };
        return author.is_some_and(|author| authors.contains(&author.to_lowercase().as_str()));
    }
    false
}

pub fn load_config(root: &Path) -> Result<PageConfig> {
    let path = root.join("config/official-pages.yaml");
    if !path.try_exists()? {
        return Ok(PageConfig {
            schema_version: 1,
            pages: vec![],
        });
    }
    let config: PageConfig = store::read_yaml(&path)?;
    ensure!(
        config.schema_version == 1,
        "unsupported official page config version"
    );
    ensure!(
        config.pages.len() <= 200,
        "official pages exceed the 200 page request budget"
    );
    let mut ids = BTreeSet::new();
    let mut urls = BTreeSet::new();
    for source in &config.pages {
        validate_source(source)?;
        ensure!(
            ids.insert(&source.id),
            "duplicate official page id {}",
            source.id
        );
        ensure!(
            urls.insert(&source.url),
            "duplicate official page URL {}",
            source.url
        );
    }
    Ok(config)
}

fn validate_source(source: &PageSource) -> Result<()> {
    ensure!(
        !source.id.is_empty()
            && source
                .id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
        "invalid official page id {}",
        source.id
    );
    ensure!(
        is_official_url(&source.vendor, &source.url),
        "{}: URL is not on the configured vendor's official domains",
        source.id
    );
    ensure!(
        !source.records.is_empty(),
        "{}: explicit model records required",
        source.id
    );
    let unique: BTreeSet<_> = source.records.iter().collect();
    ensure!(
        unique.len() == source.records.len(),
        "{}: duplicate model binding",
        source.id
    );
    Ok(())
}

fn validate_bindings(source: &PageSource, registry: &Registry) -> Result<()> {
    for id in &source.records {
        let record = registry
            .records()
            .find(|r| r.id == *id)
            .with_context(|| format!("{}: unknown model binding {id}", source.id))?;
        ensure!(
            record.kind == RecordKind::Model,
            "{}: document requires a model record, got {id}",
            source.id
        );
    }
    Ok(())
}

pub fn load(root: &Path) -> Result<DocumentCatalog> {
    let path = root.join("data/documents/official.yaml");
    if !path.try_exists()? {
        return Ok(DocumentCatalog {
            schema_version: 1,
            documents: vec![],
        });
    }
    store::read_yaml(&path)
}

pub fn extract(source: &PageSource, bytes: &[u8]) -> Result<Extraction> {
    let page = page::Page::parse(source, bytes)?;
    let mut out = page::metadata(source, &page)?;
    out.extractor_version = EXTRACTOR_VERSION;
    west::extract(source, &page, &mut out)?;
    asia::extract(source, &page, &mut out)?;
    details_west::extract(source, &page, &mut out)?;
    details_asia::extract(source, &page, &mut out)?;
    out.claims.sort_by(|a, b| {
        (&a.record, &a.property, &a.evidence.locator).cmp(&(
            &b.record,
            &b.property,
            &b.evidence.locator,
        ))
    });
    if out.claims.is_empty() && out.passages.is_empty() {
        out.warnings.push("No model field extracted; page metadata and links do not establish model capabilities.".into());
    }
    out.warnings.sort();
    out.warnings.dedup();
    Ok(out)
}

pub fn validate(
    catalog: &DocumentCatalog,
    registry: &Registry,
    cache: Option<&Path>,
) -> Result<()> {
    ensure!(
        catalog.schema_version == 1,
        "unsupported document catalog version"
    );
    let mut ids = BTreeSet::new();
    for document in &catalog.documents {
        let source = &document.source;
        validate_source(source)?;
        validate_bindings(source, registry)?;
        ensure!(ids.insert(&source.id), "duplicate document {}", source.id);
        OffsetDateTime::parse(&document.checked_at, &Rfc3339)
            .context("invalid document checked_at")?;
        ensure!(
            document.content.is_some() || document.error.is_some(),
            "{}: document has neither content nor failure",
            source.id
        );
        if let Some(content) = &document.content {
            let snapshot = &content.snapshot;
            validate_snapshot(source, snapshot)?;
            ensure!(
                content.extraction.extractor_version == EXTRACTOR_VERSION,
                "{}: extraction version changed; run documents reparse or sync documents again",
                source.id
            );
            for claim in &content.extraction.claims {
                ensure!(
                    source.records.contains(&claim.record),
                    "{}: claim escapes explicit model bindings",
                    source.id
                );
                ensure!(
                    !claim.property.is_empty()
                        && !claim.value.is_null()
                        && !claim.evidence.locator.is_empty()
                        && !claim.evidence.excerpt.is_empty()
                        && claim.evidence.excerpt.chars().count() <= 240,
                    "{}: incomplete claim evidence",
                    source.id
                );
            }
            ensure!(
                content.extraction.passages.len() <= content::MAX_PASSAGES,
                "{}: too many selected passages",
                source.id
            );
            let mut passage_chars = 0;
            for passage in &content.extraction.passages {
                let chars = passage.text.chars().count();
                passage_chars += chars;
                ensure!(
                    chars > 0
                        && chars <= content::MAX_PASSAGE_CHARS
                        && passage.text == passage.text.trim()
                        && !passage.evidence.locator.is_empty()
                        && !passage.evidence.excerpt.is_empty()
                        && passage.evidence.excerpt.chars().count() <= 240,
                    "{}: invalid selected passage",
                    source.id
                );
                ensure!(
                    passage.records.iter().all(|id| source.records.contains(id)),
                    "{}: passage escapes explicit model bindings",
                    source.id
                );
            }
            ensure!(
                passage_chars <= content::MAX_TOTAL_PASSAGE_CHARS,
                "{}: selected passage text exceeds page budget",
                source.id
            );
            for claim in &content.extraction.claims {
                if let ClaimScope::Interface { id } = &claim.scope {
                    let url =
                        Url::parse(id).context("invalid interface documentation reference")?;
                    ensure!(
                        url.scheme() == "https"
                            && url.host_str().is_some()
                            && url.username().is_empty()
                            && url.password().is_none(),
                        "invalid interface documentation reference"
                    );
                }
            }
            for link in &content.extraction.links {
                ensure!(
                    is_official_url(&source.vendor, &link.url),
                    "{}: unrelated link classified official",
                    source.id
                );
            }
            if let Some(cache) = cache {
                let bytes = read_raw(cache, &snapshot.sha256)?;
                ensure!(
                    extract(source, &bytes)? == content.extraction,
                    "{}: extracted document metadata differs from snapshot replay",
                    source.id
                );
            }
        }
    }
    Ok(())
}

fn validate_snapshot(source: &PageSource, snapshot: &crate::model::Snapshot) -> Result<()> {
    ensure!(
        snapshot.schema_version == 1
            && snapshot.source == format!("official-{}", source.vendor)
            && snapshot.url == source.url
            && snapshot.id == format!("{}:{}", snapshot.source, snapshot.sha256),
        "{}: snapshot identity mismatch",
        source.id
    );
    ensure!(
        snapshot.sha256.len() == 64
            && snapshot
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "invalid document hash"
    );
    OffsetDateTime::parse(&snapshot.retrieved_at, &Rfc3339)
        .context("invalid document retrieved_at")?;
    Ok(())
}

/// Rebuild selected content from verified local response bodies, preserving observation dates.
pub fn reparse(root: &Path) -> Result<DocumentCatalog> {
    let config = load_config(root)?;
    let registry = registry::load(root)?;
    let mut catalog = load(root)?;
    ensure!(
        catalog.schema_version == 1,
        "unsupported document catalog version"
    );
    validate_configuration(&config, &catalog, &registry)?;
    for document in &mut catalog.documents {
        if let Some(content) = &mut document.content {
            validate_snapshot(&document.source, &content.snapshot)?;
            let bytes = read_raw(&root.join(".cache"), &content.snapshot.sha256)?;
            content.extraction = extract(&document.source, &bytes)?;
        }
    }
    // Finish every parse and validation before replacing the saved catalog.
    validate(&catalog, &registry, None)?;
    store::write_yaml(&root.join("data/documents/official.yaml"), &catalog)?;
    Ok(catalog)
}

pub fn validate_configuration(
    config: &PageConfig,
    catalog: &DocumentCatalog,
    registry: &Registry,
) -> Result<()> {
    for source in &config.pages {
        validate_bindings(source, registry)?;
    }
    for doc in &catalog.documents {
        ensure!(
            config.pages.iter().any(|source| source == &doc.source),
            "{}: saved document differs from configuration; sync documents again",
            doc.source.id
        );
    }
    Ok(())
}

pub async fn sync(root: &Path, vendor: Option<&str>) -> Result<DocumentCatalog> {
    let config = load_config(root)?;
    ensure!(!config.pages.is_empty(), "no official pages configured");
    if let Some(vendor) = vendor {
        ensure!(
            config.pages.iter().any(|p| p.vendor == vendor),
            "unknown official vendor: {vendor}"
        );
    }
    let registry = registry::load(root)?;
    for source in &config.pages {
        validate_bindings(source, &registry)?;
    }
    let old = load(root)?;
    if let Some(vendor) = vendor {
        for document in &old.documents {
            if document.source.vendor != vendor {
                ensure!(
                    config.pages.contains(&document.source),
                    "{}: an unselected vendor's configuration changed; run a full sync with documents sync",
                    document.source.id
                );
            }
        }
    }
    let mut old: BTreeMap<_, _> = old
        .documents
        .into_iter()
        .map(|d| (d.source.id.clone(), d))
        .collect();
    let cache = root.join(".cache");
    let fetcher = Fetcher::new(&cache)?;
    let mut documents = Vec::new();
    for source in config.pages {
        let previous = old.remove(&source.id).filter(|d| d.source == source);
        if vendor.is_some_and(|v| source.vendor != v) {
            if let Some(previous) = previous {
                documents.push(previous);
            }
            continue;
        }
        eprintln!("official: {}", source.id);
        let checked_at = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let result = async {
            let (snapshot, bytes, content_type) = fetcher
                .get_document(&format!("official-{}", source.vendor), &source.url)
                .await?;
            let extraction = extract(&source, &bytes)?;
            Ok::<_, anyhow::Error>(PageContent {
                snapshot,
                content_type,
                extraction,
            })
        }
        .await;
        let (content, error) = match result {
            Ok(content) => (Some(content), None),
            Err(error) => {
                eprintln!("{}: {error:#}", source.id);
                (previous.and_then(|d| d.content), Some(format!("{error:#}")))
            }
        };
        documents.push(PageDocument {
            source,
            checked_at,
            error,
            content,
        });
    }
    documents.sort_by(|a, b| a.source.id.cmp(&b.source.id));
    let catalog = DocumentCatalog {
        schema_version: 1,
        documents,
    };
    // Retained pages may have no local body in a fresh clone; only --evidence requires it.
    validate(&catalog, &registry, None)?;
    store::write_yaml(&root.join("data/documents/official.yaml"), &catalog)?;
    Ok(catalog)
}

fn read_raw(root: &Path, hash: &str) -> Result<Vec<u8>> {
    let path = root.join("documents").join(format!("{hash}.bin"));
    let file =
        fs::File::open(&path).with_context(|| format!("open document {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_DOCUMENT_BYTES,
        "document exceeds 8 MiB limit"
    );
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_DOCUMENT_BYTES,
        "document exceeds 8 MiB limit"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == hash,
        "{}: document SHA-256 mismatch",
        path.display()
    );
    std::str::from_utf8(&bytes).context("document is not UTF-8")?;
    Ok(bytes)
}

pub fn copy_evidence(
    catalog: &DocumentCatalog,
    registry: &Registry,
    source: &Path,
    destination: &Path,
) -> Result<usize> {
    // Validate the complete input before replacing any destination document.
    validate(catalog, registry, Some(source))?;
    let hashes: BTreeSet<_> = catalog
        .documents
        .iter()
        .filter_map(|d| d.content.as_ref())
        .map(|c| c.snapshot.sha256.as_str())
        .collect();
    let mut written = 0;
    for hash in hashes {
        written += usize::from(store::write_bytes(
            &destination.join("documents").join(format!("{hash}.bin")),
            &read_raw(source, hash)?,
        )?);
    }
    validate(catalog, registry, Some(destination))?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_hosts_require_vendor_and_repository_namespace() {
        assert!(is_official_url(
            "openai",
            "https://developers.openai.com/api/docs/models/gpt-4.1"
        ));
        assert!(is_official_url(
            "qwen",
            "https://huggingface.co/Qwen/Qwen3-8B/raw/abc/README.md"
        ));
        assert!(is_official_url(
            "meta",
            "https://raw.githubusercontent.com/meta-llama/llama-models/main/MODEL_CARD.md"
        ));
        for url in [
            "https://openai.com.attacker.example/x",
            "http://openai.com/x",
            "https://secret@openai.com/x",
            "https://openai.com:8443/x",
            "https://huggingface.co/community/gpt-4.1",
        ] {
            assert!(!is_official_url("openai", url), "{url}");
        }
        assert!(is_official_url(
            "qwen",
            "https://huggingface.co/collections/Qwen/qwen3"
        ));
        assert!(!is_official_url(
            "qwen",
            "https://huggingface.co/collections/other/qwen3"
        ));
        assert!(!is_official_url(
            "qwen",
            "https://huggingface.co/other/Qwen3-8B"
        ));
        assert!(!is_official_url(
            "meta",
            "https://github.com/other/llama-models"
        ));
    }

    #[test]
    fn config_rejects_duplicates_and_foreign_authors() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = PageSource {
            id: "qwen-card".into(),
            vendor: "qwen".into(),
            kind: DocumentKind::ModelCard,
            url: "https://huggingface.co/Qwen/Qwen3-8B/raw/abc/README.md".into(),
            format: Format::Markdown,
            records: vec!["hf:model:qwen".into()],
        };
        let mut config = PageConfig {
            schema_version: 1,
            pages: vec![source.clone(), source],
        };
        let path = root.path().join("config/official-pages.yaml");
        store::write_yaml(&path, &config)?;
        assert!(
            load_config(root.path())
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
        config.pages.pop();
        config.pages[0].url = "https://huggingface.co/unrelated/Qwen3-8B".into();
        store::write_yaml(&path, &config)?;
        assert!(
            load_config(root.path())
                .unwrap_err()
                .to_string()
                .contains("official domains")
        );
        Ok(())
    }
}

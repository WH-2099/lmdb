mod asia;
pub mod compare;
mod content;
mod details_asia;
mod details_west;
mod directories;
mod expanded_asia;
mod expanded_modal;
pub mod model;
pub mod page;
mod pdf;
mod repository;
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

pub const EXTRACTOR_VERSION: u32 = 4;
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
                "www-cdn.anthropic.com",
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
        "moonshot" => (
            &[
                "www.kimi.ai",
                "www.kimi.com",
                "platform.kimi.ai",
                "www.moonshot.ai",
            ],
            &["moonshotai"],
        ),
        "zai" => (&["z.ai", "docs.z.ai"], &["zai-org"]),
        "minimax" => (&["www.minimax.io", "platform.minimax.io"], &["minimaxai"]),
        "baidu" => (&["ernie.baidu.com"], &["baidu"]),
        "bfl" => (&["bfl.ai", "docs.bfl.ai"], &["black-forest-labs"]),
        "stability" => (&["stability.ai"], &["stabilityai"]),
        "wan" => (&["www.alibabacloud.com"], &["wan-ai", "wan-video"]),
        "bytedance" => (&["seed.bytedance.com"], &["bytedance", "bytedance-seed"]),
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
    repository::validate(source)?;
    if source.kind == DocumentKind::Directory {
        ensure!(
            source.records.is_empty(),
            "{}: directory entries must remain unbound candidates",
            source.id
        );
    } else {
        ensure!(
            source.records.len() == 1,
            "{}: exactly one explicit model record is required; directory entries remain unbound",
            source.id
        );
    }
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
    ensure!(
        bytes.len() as u64 <= MAX_DOCUMENT_BYTES,
        "document exceeds 8 MiB limit"
    );
    if source.format == Format::Pdf {
        return pdf::extract(source, bytes);
    }
    let page = page::Page::parse(source, bytes)?;
    let mut out = page::metadata(source, &page)?;
    out.extractor_version = EXTRACTOR_VERSION;
    west::extract(source, &page, &mut out)?;
    asia::extract(source, &page, &mut out)?;
    details_west::extract(source, &page, &mut out)?;
    details_asia::extract(source, &page, &mut out)?;
    expanded_asia::extract(source, &page, &mut out)?;
    expanded_modal::extract(source, &page, &mut out)?;
    directories::extract(source, &page, &mut out)?;
    let mut linked_urls = BTreeSet::new();
    out.links
        .retain(|link| linked_urls.insert(link.url.clone()));
    if out.links.len() > page::MAX_OFFICIAL_LINKS {
        out.warnings.push(format!("Official link limit of {} reached after embedded content extraction; additional links were omitted.", page::MAX_OFFICIAL_LINKS));
        out.links.truncate(page::MAX_OFFICIAL_LINKS);
    }
    out.claims.sort_by(|a, b| {
        (&a.record, &a.property, &a.evidence.locator).cmp(&(
            &b.record,
            &b.property,
            &b.evidence.locator,
        ))
    });
    if out.claims.is_empty() && out.passages.is_empty() && source.kind != DocumentKind::Directory {
        out.warnings.push("No model field extracted; page metadata and links do not establish model capabilities.".into());
    }
    out.warnings.sort();
    out.warnings.dedup();
    page::require_content(source, &page, &out)?;
    Ok(out)
}

fn extract_snapshot(
    source: &PageSource,
    snapshot: &crate::model::Snapshot,
    bytes: &[u8],
) -> Result<Extraction> {
    let mut resolved = source.clone();
    // Relative links belong to the fetched revision, including when replaying older evidence.
    resolved.url = snapshot.url.clone();
    extract(&resolved, bytes)
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
        ensure!(
            document.error_kind.is_none() || document.error.is_some(),
            "{}: error kind requires a diagnostic",
            source.id
        );
        if let Some(snapshot) = &document.failed_snapshot {
            ensure!(
                document.error.is_some(),
                "{}: rejected snapshot requires a diagnostic",
                source.id
            );
            validate_snapshot(source, snapshot)?;
            if let Some(cache) = cache {
                let bytes = read_raw(cache, &snapshot.sha256)?;
                ensure!(
                    extract_snapshot(source, snapshot, &bytes).is_err(),
                    "{}: rejected snapshot now extracts successfully; run documents reparse",
                    source.id
                );
            }
        }
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
            ensure!(
                content.extraction.links.len() <= page::MAX_OFFICIAL_LINKS,
                "{}: too many official links",
                source.id
            );
            let entries = &content.extraction.catalog_entries;
            ensure!(
                entries.is_empty() || source.kind == DocumentKind::Directory,
                "{}: catalog entries require a directory source",
                source.id
            );
            ensure!(
                entries.len() <= 500,
                "{}: directory exceeds the 500 entry limit",
                source.id
            );
            let mut urls = BTreeSet::new();
            for entry in entries {
                ensure!(
                    !entry.label.trim().is_empty()
                        && is_official_url(&source.vendor, &entry.url)
                        && urls.insert(&entry.url)
                        && entry
                            .model_id
                            .as_ref()
                            .is_none_or(|id| !id.trim().is_empty())
                        && !entry.evidence.locator.is_empty()
                        && !entry.evidence.excerpt.is_empty()
                        && entry.evidence.excerpt.chars().count() <= 240,
                    "{}: invalid or duplicate directory entry",
                    source.id
                );
            }
            if let Some(cache) = cache {
                let bytes = read_raw(cache, &snapshot.sha256)?;
                ensure!(
                    extract_snapshot(source, snapshot, &bytes)? == content.extraction,
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
            && is_official_url(&source.vendor, &snapshot.url)
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
    repository::validate_snapshot(source, snapshot)?;
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
    for document in &mut catalog.documents {
        if let Some(configured) = config
            .pages
            .iter()
            .find(|source| source.id == document.source.id)
        {
            migrate_source_definition(document, configured)?;
        }
    }
    validate_configuration(&config, &catalog, &registry)?;
    for document in &mut catalog.documents {
        if let Some(mut content) = document.content.take() {
            validate_snapshot(&document.source, &content.snapshot)?;
            let bytes = read_raw(&root.join(".cache"), &content.snapshot.sha256)?;
            match extract_snapshot(&document.source, &content.snapshot, &bytes) {
                Ok(extraction) => {
                    content.extraction = extraction;
                    document.content = Some(content);
                }
                Err(error) => {
                    if document.error.is_none() {
                        document.error_kind = Some(DocumentErrorKind::Extraction);
                    }
                    if document.failed_snapshot.is_none() {
                        document.failed_snapshot = Some(content.snapshot);
                    }
                    let diagnostic = format!("cached document rejected during reparse: {error:#}");
                    document.error = Some(match document.error.take() {
                        Some(previous) => format!("{previous}; {diagnostic}"),
                        None => diagnostic,
                    });
                }
            }
        }
        if let Some(snapshot) = document.failed_snapshot.as_ref() {
            validate_snapshot(&document.source, snapshot)?;
            let bytes = read_raw(&root.join(".cache"), &snapshot.sha256)?;
            if let Ok(extraction) = extract_snapshot(&document.source, snapshot, &bytes) {
                document.content = Some(PageContent {
                    snapshot: snapshot.clone(),
                    content_type: None,
                    extraction,
                });
                document.failed_snapshot = None;
                if document.error_kind == Some(DocumentErrorKind::Extraction) {
                    document.error = None;
                    document.error_kind = None;
                }
            }
        }
    }
    // Finish every parse and validation before replacing the saved catalog.
    validate(&catalog, &registry, None)?;
    store::write_yaml(&root.join("data/documents/official.yaml"), &catalog)?;
    Ok(catalog)
}

fn migrate_source_definition(document: &mut PageDocument, configured: &PageSource) -> Result<()> {
    if document.source == *configured {
        return Ok(());
    }
    let old = &document.source;
    let mut expected = old.clone();
    if document.content.is_none()
        && document.failed_snapshot.is_none()
        && old.format == Format::Html
        && configured.format == Format::Pdf
    {
        expected.format = Format::Pdf;
        if expected == *configured {
            document.source = configured.clone();
            return Ok(());
        }
    }
    // A legacy immutable URL already proves the old commit; migration changes no observation date.
    ensure!(
        old.repository.is_none()
            && matches!(
                configured.repository.as_ref().map(|repo| &repo.revision),
                Some(RepositoryRevision::Tracked { .. })
            ),
        "{}: source changed beyond a legacy repository migration; sync documents again",
        old.id
    );
    expected.url = configured.url.clone();
    expected.repository = configured.repository.clone();
    ensure!(
        expected == *configured && repository::same_document_url(&old.url, &configured.url),
        "{}: repository migration changed document identity or path; sync documents again",
        old.id
    );
    let old_url = Url::parse(&old.url)?;
    let sha = old_url
        .path_segments()
        .and_then(|mut segments| segments.nth(3))
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .context("legacy repository migration requires an immutable 40-digit commit URL")?;
    for snapshot in document
        .content
        .as_mut()
        .map(|content| &mut content.snapshot)
        .into_iter()
        .chain(document.failed_snapshot.as_mut())
    {
        ensure!(
            snapshot.url == old.url
                && snapshot
                    .upstream_revision
                    .as_deref()
                    .is_none_or(|revision| revision == sha),
            "{}: legacy snapshot does not match its configured commit",
            old.id
        );
        snapshot.upstream_revision = Some(sha.into());
        validate_snapshot(configured, snapshot)?;
    }
    document.source = configured.clone();
    Ok(())
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
        let response = async {
            let (url, revision) = repository::fetch_target(&source, &fetcher).await?;
            let (mut snapshot, bytes, content_type) = fetcher
                .get_document_with_redirects(&format!("official-{}", source.vendor), &url, |url| {
                    is_official_url(&source.vendor, url)
                })
                .await?;
            snapshot.upstream_revision = revision;
            validate_snapshot(&source, &snapshot)?;
            Ok((snapshot, bytes, content_type))
        }
        .await;
        documents.push(refresh_response(
            source, previous, checked_at, response, &cache,
        )?);
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

fn refresh_response(
    source: PageSource,
    previous: Option<PageDocument>,
    checked_at: String,
    response: Result<(crate::model::Snapshot, Vec<u8>, Option<String>)>,
    cache: &Path,
) -> Result<PageDocument> {
    let previous = previous.filter(|document| document.source == source);
    let mut failed_snapshot = previous
        .as_ref()
        .and_then(|document| document.failed_snapshot.clone());
    let mut error_kind = DocumentErrorKind::Fetch;
    let result = response.and_then(|(snapshot, bytes, content_type)| {
        validate_snapshot(&source, &snapshot)?;
        error_kind = DocumentErrorKind::Extraction;
        failed_snapshot = Some(snapshot.clone());
        let extraction = extract_snapshot(&source, &snapshot, &bytes)?;
        failed_snapshot = None;
        Ok(PageContent {
            snapshot,
            content_type,
            extraction,
        })
    });
    let (content, error) = match result {
        Ok(content) => (Some(content), None),
        Err(error) => {
            let mut diagnostic = format!("{error:#}");
            eprintln!("{}: {diagnostic}", source.id);
            let mut retained = previous.and_then(|document| document.content);
            if let Some(content) = retained.as_mut()
                && content.extraction.extractor_version != EXTRACTOR_VERSION
            {
                validate_snapshot(&source, &content.snapshot)?;
                let bytes = read_raw(cache, &content.snapshot.sha256).with_context(|| {
                    format!("{}: cached body required to upgrade retained extraction after fetch failure", source.id)
                })?;
                match extract_snapshot(&source, &content.snapshot, &bytes) {
                    Ok(extraction) => content.extraction = extraction,
                    Err(error) => {
                        if failed_snapshot.is_none() {
                            failed_snapshot = Some(content.snapshot.clone());
                        }
                        diagnostic.push_str(&format!("; retained body rejected: {error:#}"));
                        retained = None;
                    }
                }
            }
            (retained, Some(diagnostic))
        }
    };
    Ok(PageDocument {
        error_kind: error.as_ref().map(|_| error_kind),
        source,
        checked_at,
        error,
        content,
        failed_snapshot,
    })
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
        .flat_map(|d| {
            d.content
                .as_ref()
                .map(|c| &c.snapshot)
                .into_iter()
                .chain(d.failed_snapshot.as_ref())
        })
        .map(|snapshot| snapshot.sha256.as_str())
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
            repository: None,
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

    #[test]
    fn content_pages_cannot_fan_out_to_multiple_models_and_directories_cannot_bind_models() {
        let mut source = PageSource {
            repository: None,
            id: "microsoft-phi4-card".into(),
            vendor: "microsoft".into(),
            kind: DocumentKind::ModelCard,
            url: "https://huggingface.co/microsoft/phi-4/raw/main/README.md".into(),
            format: Format::Markdown,
            records: vec![
                "huggingface:model:microsoft/phi-4".into(),
                "huggingface:model:microsoft/Phi-4-mini-instruct".into(),
            ],
        };
        assert!(validate_source(&source).is_err());
        source.records.pop();
        assert!(validate_source(&source).is_ok());
        source.kind = DocumentKind::Directory;
        assert!(validate_source(&source).is_err());
        source.records.clear();
        assert!(validate_source(&source).is_ok());
    }
    fn model_source() -> PageSource {
        serde_json::from_value(serde_json::json!({
            "id": "openai-gpt41-model", "vendor": "openai", "kind": "model_page",
            "url": "https://developers.openai.com/api/docs/models/gpt-4.1",
            "format": "html", "records": ["modelsdev:model:openai/gpt-4.1"]
        }))
        .unwrap()
    }

    fn response(
        source: &PageSource,
        bytes: &[u8],
    ) -> (crate::model::Snapshot, Vec<u8>, Option<String>) {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        (
            crate::model::Snapshot {
                schema_version: 1,
                id: format!("official-{}:{sha256}", source.vendor),
                source: format!("official-{}", source.vendor),
                url: source.url.clone(),
                retrieved_at: "2026-10-19T00:00:00Z".into(),
                sha256,
                upstream_revision: None,
            },
            bytes.to_vec(),
            Some("text/html".into()),
        )
    }

    #[test]
    fn shell_or_missing_identity_retains_previous_content_and_rejected_evidence() -> Result<()> {
        let source = model_source();
        let cache = tempfile::tempdir()?;
        let raw = br"<main><h1>GPT-4.1</h1><div>1,047,576 context window</div><div>32,768 max output tokens</div></main>";
        let previous = refresh_response(
            source.clone(),
            None,
            "2026-10-18T00:00:00Z".into(),
            Ok(response(&source, raw)),
            cache.path(),
        )?;
        assert!(previous.error.is_none());
        for raw in [
            b"<html><head><title>GPT-4.1</title></head><body><div id='app'></div><script>load()</script></body></html>".as_slice(),
            b"<main><h1>Another model</h1><div>1,047,576 context window</div><div>32,768 max output tokens</div></main>".as_slice(),
        ] {
            let fetched = response(&source, raw);
            let rejected = fetched.0.clone();
            let refreshed = refresh_response(source.clone(), Some(previous.clone()), "2026-10-19T00:00:00Z".into(), Ok(fetched), cache.path())?;
            assert_eq!(refreshed.content, previous.content);
            assert_eq!(refreshed.failed_snapshot, Some(rejected));
            assert!(refreshed.error.as_deref().unwrap().contains("identity or content structure was absent"));
            assert_ne!(refreshed.checked_at, previous.checked_at);
        }
        Ok(())
    }

    #[test]
    fn descriptive_release_and_directory_succeed_without_model_claims() -> Result<()> {
        let mut source = model_source();
        source.kind = DocumentKind::ReleaseBlog;
        source.id = "anthropic-claude4-blog".into();
        source.vendor = "anthropic".into();
        source.url = "https://www.anthropic.com/news/claude-4".into();
        let out = extract(&source, "<main><article><p class='post-text'>Today, we’re introducing Claude 4.</p></article></main>".as_bytes())?;
        assert!(out.claims.is_empty());
        assert_eq!(out.passages.len(), 1);
        source.kind = DocumentKind::Directory;
        source.id = "google-model-directory".into();
        source.vendor = "google".into();
        source.url = "https://ai.google.dev/gemini-api/docs/models".into();
        source.records.clear();
        assert!(extract(&source, b"<main><h1>Models</h1></main>").is_err());
        let out = extract(
            &source,
            include_bytes!("../../tests/fixtures/official/google-directory.html"),
        )?;
        assert!(out.claims.is_empty());
        assert!(!out.catalog_entries.is_empty());
        Ok(())
    }

    #[test]
    fn changed_source_cannot_reuse_previous_body() -> Result<()> {
        let mut source = model_source();
        let cache = tempfile::tempdir()?;
        let raw = br"<main><h1>GPT-4.1</h1><div>1,047,576 context window</div><div>32,768 max output tokens</div></main>";
        let previous = refresh_response(
            source.clone(),
            None,
            "2026-10-18T00:00:00Z".into(),
            Ok(response(&source, raw)),
            cache.path(),
        )?;
        source.url = "https://developers.openai.com/api/docs/models/gpt-4.1-mini".into();
        let refreshed = refresh_response(
            source,
            Some(previous),
            "2026-10-19T00:00:00Z".into(),
            Err(anyhow::anyhow!("network failed")),
            cache.path(),
        )?;
        assert!(refreshed.content.is_none());
        assert!(refreshed.failed_snapshot.is_none());
        Ok(())
    }
    #[test]
    fn legacy_repository_migration_preserves_exact_commit_and_replay() -> Result<()> {
        let sha = "1234567890123456789012345678901234567890";
        let source: PageSource = serde_json::from_value(serde_json::json!({
            "id":"qwen-card", "vendor":"qwen", "kind":"model_card",
            "url":format!("https://huggingface.co/Qwen/Qwen3-8B/raw/{sha}/README.md"),
            "format":"markdown", "records":["huggingface:model:Qwen/Qwen3-8B"]
        }))?;
        let raw = b"---\nlicense: apache-2.0\n---\n# Qwen3-8B\n\n[Configuration](config.json)\n";
        let fetched = response(&source, raw);
        let original_snapshot = fetched.0.clone();
        let cache = tempfile::tempdir()?;
        let mut previous = refresh_response(
            source.clone(),
            None,
            "2026-10-18T00:00:00Z".into(),
            Ok(fetched),
            cache.path(),
        )?;
        let old_extraction = previous.content.as_ref().unwrap().extraction.clone();
        let mut configured = source.clone();
        configured.url = "https://huggingface.co/Qwen/Qwen3-8B/raw/main/README.md".into();
        configured.repository = Some(RepositorySource {
            repo: "Qwen/Qwen3-8B".into(),
            path: "README.md".into(),
            revision: RepositoryRevision::Tracked {
                reference: "main".into(),
            },
        });
        migrate_source_definition(&mut previous, &configured)?;
        assert_eq!(previous.source, configured);
        assert_eq!(previous.checked_at, "2026-10-18T00:00:00Z");
        let snapshot = &previous.content.as_ref().unwrap().snapshot;
        assert_eq!(snapshot.url, original_snapshot.url);
        assert_eq!(snapshot.retrieved_at, original_snapshot.retrieved_at);
        assert_eq!(snapshot.sha256, original_snapshot.sha256);
        assert_eq!(snapshot.upstream_revision.as_deref(), Some(sha));
        assert_eq!(
            extract_snapshot(&configured, snapshot, raw)?,
            old_extraction
        );
        assert!(old_extraction.links[0].url.contains(sha));
        Ok(())
    }

    #[test]
    fn repository_migration_rejects_changed_subject_path_and_unproven_revision() -> Result<()> {
        let configured: PageSource = serde_json::from_value(serde_json::json!({
            "id":"qwen-card", "vendor":"qwen", "kind":"model_card",
            "url":"https://huggingface.co/Qwen/Qwen3-8B/raw/main/README.md",
            "format":"markdown", "records":["huggingface:model:Qwen/Qwen3-8B"],
            "repository":{"repo":"Qwen/Qwen3-8B", "path":"README.md", "revision":{"mode":"tracked", "reference":"main"}}
        }))?;
        let mut legacy = configured.clone();
        legacy.repository = None;
        legacy.url = "https://huggingface.co/Qwen/Qwen3-8B/raw/1234567890123456789012345678901234567890/README.md".into();
        let old = PageDocument {
            error_kind: None,
            source: legacy,
            checked_at: "2026-10-18T00:00:00Z".into(),
            content: None,
            failed_snapshot: None,
            error: Some("failed".into()),
        };
        for change in ["subject", "path", "revision", "snapshot"] {
            let mut document = old.clone();
            match change {
                "subject" => {
                    document.source.records = vec!["huggingface:model:Qwen/Qwen3-4B".into()]
                }
                "path" => {
                    document.source.url = document.source.url.replace("README.md", "OTHER.md")
                }
                "revision" => {
                    document.source.url = document
                        .source
                        .url
                        .replace("1234567890123456789012345678901234567890", "main")
                }
                "snapshot" => {
                    let mut snapshot = response(&document.source, b"bad").0;
                    snapshot.upstream_revision = Some("f".repeat(40));
                    document.failed_snapshot = Some(snapshot);
                }
                _ => unreachable!(),
            }
            assert!(
                migrate_source_definition(&mut document, &configured).is_err(),
                "{change}"
            );
        }
        Ok(())
    }

    #[test]
    fn format_migration_only_changes_sources_without_a_saved_response() -> Result<()> {
        let mut source = model_source();
        let mut document = PageDocument {
            error_kind: None,
            source: source.clone(),
            checked_at: "2026-10-18T00:00:00Z".into(),
            content: None,
            failed_snapshot: None,
            error: Some("HTTP 307".into()),
        };
        source.format = Format::Pdf;
        let mut with_body = document.clone();
        with_body.failed_snapshot = Some(response(&document.source, b"not PDF").0);
        assert!(migrate_source_definition(&mut with_body, &source).is_err());
        migrate_source_definition(&mut document, &source)?;
        assert_eq!(document.source.format, Format::Pdf);
        assert_eq!(document.error.as_deref(), Some("HTTP 307"));
        Ok(())
    }
}

use std::collections::BTreeSet;

use anyhow::Result;
use reqwest::Url;
use scraper::{ElementRef, Selector};

use super::page::{Page, text};
use super::{CatalogEntry, DocumentKind, Extraction, PageSource};

const MAX_ENTRIES: usize = 500;

pub fn extract(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    if source.kind != DocumentKind::Directory {
        return Ok(());
    }
    let selector = match source.id.as_str() {
        "google-model-directory" => ".devsite-article-body a[href]",
        "qwen-blog-directory" => "article.post-entry a.entry-link[href]",
        "anthropic-model-directory" => "main a[href]",
        "mistral-model-directory" => "article[data-model-card] a[href]",
        "kimi-model-directory" => "main a[href]",
        "zai-model-directory" => "li > a[href]:first-child",
        "seed-model-directory" => "main section a[href]",
        "stability-model-directory" => ".table_component table tbody tr a[href]",
        "wan-model-directory" => "table tbody tr a[href]",
        _ => {
            out.warnings
                .push("No directory extractor is configured for this page.".into());
            return Ok(());
        }
    };
    let base = Url::parse(&source.url)?;
    let mut seen = BTreeSet::new();
    for anchor in page.select(selector) {
        if anchor.ancestors().filter_map(ElementRef::wrap).any(|node| {
            matches!(node.value().name(), "nav" | "footer" | "aside")
                || matches!(node.attr("role"), Some("navigation" | "contentinfo"))
        }) {
            continue;
        }
        let href = anchor.attr("href").unwrap_or_default().trim();
        if href.is_empty() || href.starts_with('#') {
            continue;
        }
        let Ok(mut url) = base.join(href) else {
            continue;
        };
        url.set_fragment(None);
        if !super::is_official_url(&source.vendor, url.as_str())
            || !is_entry(&source.id, &url)
            || !seen.insert(url.clone())
        {
            continue;
        }
        if out.catalog_entries.len() == MAX_ENTRIES {
            out.warnings.push(format!(
                "Directory entry limit of {MAX_ENTRIES} reached; additional entries were omitted."
            ));
            break;
        }
        let label = entry_label(&source.id, anchor);
        out.catalog_entries.push(CatalogEntry {
            label: if label.is_empty() {
                url.to_string()
            } else {
                label
            },
            url: url.into(),
            model_id: None,
            evidence: page.attribute_evidence(anchor, "href"),
        });
    }
    if out.catalog_entries.is_empty() {
        out.warnings
            .push("No directory entries matched the expected page structure and URL scope.".into());
    }
    out.warnings.push(
        "Directory entries cover only this fetched page and remain unbound candidates for review; additional pages and interactive results are not enumerated."
            .into(),
    );
    Ok(())
}

fn is_entry(id: &str, url: &Url) -> bool {
    let path = url.path().trim_end_matches('/');
    match id {
        "google-model-directory" => {
            url.host_str() == Some("ai.google.dev")
                && path
                    .strip_prefix("/gemini-api/docs/models/")
                    .is_some_and(|slug| !slug.is_empty())
        }
        "qwen-blog-directory" => {
            url.host_str() == Some("qwenlm.github.io")
                && path
                    .strip_prefix("/blog/")
                    .is_some_and(|slug| !slug.is_empty() && !slug.contains('/') && slug != "page")
        }
        "anthropic-model-directory" => {
            url.host_str() == Some("www.anthropic.com")
                && path.strip_prefix("/claude-").is_some_and(|slug| {
                    !slug.contains('/')
                        && slug.split('-').any(|part| {
                            !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
                        })
                })
        }
        "mistral-model-directory" => {
            url.host_str() == Some("docs.mistral.ai")
                && path
                    .strip_prefix("/models/")
                    .is_some_and(|slug| !slug.is_empty())
        }
        "kimi-model-directory" => {
            url.host_str() == Some("www.kimi.ai")
                && (path
                    .strip_prefix("/ai-models/")
                    .is_some_and(|slug| !slug.is_empty() && !slug.contains('/'))
                    || path == "/resources/kimi-k2-7-code")
        }
        "zai-model-directory" => {
            url.host_str() == Some("docs.z.ai")
                && ["/guides/llm/", "/guides/vlm/"].iter().any(|prefix| {
                    path.strip_prefix(prefix)
                        .is_some_and(|slug| !slug.is_empty() && !slug.contains('/'))
                })
        }
        "seed-model-directory" => {
            url.host_str() == Some("seed.bytedance.com")
                && path.strip_prefix("/en/").is_some_and(|slug| {
                    !slug.is_empty()
                        && !slug.contains('/')
                        && !matches!(slug, "models" | "blog" | "about" | "joinus" | "opensource")
                })
        }
        "stability-model-directory" => {
            url.host_str() == Some("huggingface.co")
                && [
                    "/stabilityai/",
                    "/collections/stabilityai/",
                    "/spaces/stabilityai/",
                ]
                .iter()
                .any(|prefix| {
                    path.strip_prefix(prefix)
                        .is_some_and(|slug| !slug.is_empty() && !slug.contains('/'))
                })
        }
        "wan-model-directory" => {
            url.host_str() == Some("huggingface.co")
                && path
                    .strip_prefix("/Wan-AI/")
                    .is_some_and(|repo| !repo.is_empty() && !repo.contains('/'))
        }
        _ => false,
    }
}

fn entry_label(id: &str, anchor: ElementRef<'_>) -> String {
    if id == "seed-model-directory" {
        for parent in anchor
            .ancestors()
            .filter_map(ElementRef::wrap)
            .filter(|node| matches!(node.value().name(), "article" | "section"))
        {
            if let Some(label) = first_text(parent, "h1, h2, h3") {
                return label;
            }
        }
    }
    if id == "wan-model-directory"
        && let Some(row) = anchor
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find(|node| node.value().name() == "tr")
        && let Some(label) = first_text(row, "td")
    {
        return label;
    }
    if matches!(id, "qwen-blog-directory" | "mistral-model-directory")
        && let Some(article) = anchor
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find(|node| node.value().name() == "article")
        && let Some(label) = first_text(
            article,
            if id == "qwen-blog-directory" {
                ".entry-header h2"
            } else {
                "p.text-h4"
            },
        )
    {
        return label;
    }
    first_text(anchor, "h2, h3, h4")
        .or_else(|| {
            anchor.attr("aria-label").map(|value| {
                value
                    .strip_prefix("post link to ")
                    .unwrap_or(value)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        })
        .unwrap_or_else(|| text(anchor))
}

fn first_text(element: ElementRef<'_>, selector: &str) -> Option<String> {
    element
        .select(&Selector::parse(selector).expect("static selector"))
        .map(text)
        .find(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::Format;

    fn source(vendor: &str, id: &str, url: &str) -> PageSource {
        PageSource {
            repository: None,
            id: id.into(),
            vendor: vendor.into(),
            kind: DocumentKind::Directory,
            url: url.into(),
            format: Format::Html,
            records: vec![],
        }
    }

    fn run(source: &PageSource, raw: &str) -> (Page, Extraction) {
        let page = Page::parse(source, raw.as_bytes()).unwrap();
        let mut out = super::super::page::metadata(source, &page).unwrap();
        extract(source, &page, &mut out).unwrap();
        (page, out)
    }

    #[test]
    fn directory_fixtures_preserve_labels_and_resolvable_href_evidence() {
        let cases = [
            (
                source(
                    "google",
                    "google-model-directory",
                    "https://ai.google.dev/gemini-api/docs/models",
                ),
                include_str!("../../tests/fixtures/official/google-directory.html"),
                vec!["Gemini 4 Argon"],
            ),
            (
                source(
                    "anthropic",
                    "anthropic-model-directory",
                    "https://www.anthropic.com/news",
                ),
                include_str!("../../tests/fixtures/official/anthropic-directory.html"),
                vec![
                    "Introducing Claude Haiku 5.5",
                    "Introducing Claude Sonnet 5.5",
                ],
            ),
            (
                source(
                    "qwen",
                    "qwen-blog-directory",
                    "https://qwenlm.github.io/blog/",
                ),
                include_str!("../../tests/fixtures/official/qwen-directory.html"),
                vec![
                    "Qwen3Guard: Real-time Safety for Your Token Stream",
                    "Qwen-Image-Edit: Image Editing with Higher Quality and Efficiency",
                ],
            ),
            (
                source(
                    "mistral",
                    "mistral-model-directory",
                    "https://mistral.ai/research/",
                ),
                include_str!("../../tests/fixtures/official/mistral-directory.html"),
                vec!["Mistral Large 4", "Mistral Medium 3.5"],
            ),
        ];
        for (source, fixture, labels) in cases {
            let (page, out) = run(&source, fixture);
            assert_eq!(
                out.catalog_entries
                    .iter()
                    .map(|entry| entry.label.as_str())
                    .collect::<Vec<_>>(),
                labels
            );
            for entry in out.catalog_entries {
                assert!(entry.model_id.is_none());
                let locator = entry
                    .evidence
                    .locator
                    .strip_prefix("css:")
                    .unwrap()
                    .strip_suffix("@href")
                    .unwrap();
                let nodes = page.select(locator);
                assert_eq!(nodes.len(), 1);
                assert_eq!(nodes[0].attr("href"), Some(entry.evidence.excerpt.as_str()));
                assert!(super::super::is_official_url(&source.vendor, &entry.url));
            }
            assert!(out.claims.is_empty() && out.passages.is_empty());
        }
    }

    #[test]
    fn directory_rejects_navigation_and_unsafe_or_unrelated_urls() {
        let source = source(
            "google",
            "google-model-directory",
            "https://ai.google.dev/gemini-api/docs/models",
        );
        let (_, out) = run(
            &source,
            r#"<div class="devsite-article-body">
            <nav><a href="/gemini-api/docs/models/nav">Navigation</a></nav>
            <aside><a href="/gemini-api/docs/models/aside">Aside</a></aside>
            <a href="https://evil.example/gemini-api/docs/models/fake">Wrong host</a>
            <a href="https://user@ai.google.dev/gemini-api/docs/models/fake">Credentials</a>
            <a href="http://ai.google.dev/gemini-api/docs/models/fake">Plain HTTP</a>
            <a href="/gemini-api/docs/pricing">Pricing</a>
            <a href="/gemini-api/docs/models/model#one">Model</a>
            <a href="/gemini-api/docs/models/model#two">Duplicate</a>
        </div>"#,
        );
        assert_eq!(out.catalog_entries.len(), 1);
        assert_eq!(
            out.catalog_entries[0].url,
            "https://ai.google.dev/gemini-api/docs/models/model"
        );
    }

    #[test]
    fn additional_directories_keep_cards_and_download_rows_unbound() {
        let cases = [
            (
                "moonshot",
                "kimi-model-directory",
                "https://www.kimi.ai/ai-models/",
                Format::Html,
                include_str!("../../tests/fixtures/official/kimi-directory.html"),
                4,
                "Kimi K3",
            ),
            (
                "zai",
                "zai-model-directory",
                "https://docs.z.ai/llms.txt",
                Format::Markdown,
                include_str!("../../tests/fixtures/official/zai-directory.md"),
                3,
                "GLM-5.3-Flash/FlashX",
            ),
            (
                "bytedance",
                "seed-model-directory",
                "https://seed.bytedance.com/en/models",
                Format::Html,
                include_str!("../../tests/fixtures/official/seed-directory.html"),
                2,
                "Seed2.1",
            ),
            (
                "stability",
                "stability-model-directory",
                "https://stability.ai/core-models",
                Format::Html,
                include_str!("../../tests/fixtures/official/stability-directory.html"),
                24,
                "Stable Diffusion 3.5 Medium",
            ),
            (
                "wan",
                "wan-model-directory",
                "https://raw.githubusercontent.com/Wan-Video/Wan2.2/main/README.md",
                Format::Markdown,
                include_str!("../../tests/fixtures/official/wan-directory.md"),
                5,
                "T2V-A14B",
            ),
        ];
        for (vendor, id, url, format, fixture, count, first_label) in cases {
            let mut source = source(vendor, id, url);
            source.format = format;
            let (page, out) = run(&source, fixture);
            assert_eq!(out.catalog_entries.len(), count, "{id}");
            assert_eq!(out.catalog_entries[0].label, first_label, "{id}");
            for entry in out.catalog_entries {
                assert!(entry.model_id.is_none());
                let prefix = if format == Format::Markdown {
                    "markdown-rendered-css:"
                } else {
                    "css:"
                };
                let locator = entry
                    .evidence
                    .locator
                    .strip_prefix(prefix)
                    .unwrap()
                    .strip_suffix("@href")
                    .unwrap();
                assert_eq!(
                    page.select(locator)[0].attr("href"),
                    Some(entry.evidence.excerpt.as_str())
                );
            }
            assert!(out.claims.is_empty() && out.passages.is_empty());
        }
    }

    #[test]
    fn directory_bounds_candidates_and_reports_changed_structure() {
        let mut source = source(
            "google",
            "google-model-directory",
            "https://ai.google.dev/gemini-api/docs/models",
        );
        let body = (0..=MAX_ENTRIES)
            .map(|index| format!("<a href='/gemini-api/docs/models/{index}'>Model {index}</a>"))
            .collect::<String>();
        let raw = format!("<div class='devsite-article-body'>{body}</div>");
        let (_, out) = run(&source, &raw);
        assert_eq!(out.catalog_entries.len(), MAX_ENTRIES);
        assert!(
            out.warnings
                .iter()
                .any(|warning| warning.contains("additional entries were omitted"))
        );
        let (_, empty) = run(&source, "<main>Structure changed</main>");
        assert!(empty.catalog_entries.is_empty());
        assert!(
            empty
                .warnings
                .iter()
                .any(|warning| warning.contains("No directory entries matched"))
        );
        source.kind = DocumentKind::ModelPage;
        let (_, model_page) = run(&source, &raw);
        assert!(model_page.catalog_entries.is_empty());
    }
}

use std::collections::HashSet;

use anyhow::{Context, Result};
use pulldown_cmark::{Options, Parser, html};
use reqwest::Url;
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;

use super::model::{Extraction, Format, LocatedValue, OfficialLink, PageEvidence, PageSource};

pub struct Page {
    pub html: Html,
    pub raw: String,
    pub format: Format,
}

impl Page {
    pub fn parse(source: &PageSource, raw: &[u8]) -> Result<Self> {
        let raw = std::str::from_utf8(raw)
            .context("document response is not UTF-8")?
            .to_owned();
        let rendered = match source.format {
            Format::Html => raw.clone(),
            Format::Markdown => {
                let mut rendered = String::new();
                html::push_html(
                    &mut rendered,
                    Parser::new_ext(markdown_body(&raw), Options::ENABLE_TABLES),
                );
                rendered
            }
        };
        Ok(Self {
            html: Html::parse_document(&rendered),
            raw,
            format: source.format,
        })
    }

    pub fn select(&self, selector: &str) -> Vec<ElementRef<'_>> {
        Selector::parse(selector)
            .map(|selector| self.html.select(&selector).collect())
            .unwrap_or_default()
    }

    pub fn evidence(&self, element: ElementRef<'_>) -> PageEvidence {
        PageEvidence {
            locator: self.locator(element),
            excerpt: excerpt(&text(element)),
        }
    }

    pub fn attribute_evidence(&self, element: ElementRef<'_>, attribute: &str) -> PageEvidence {
        PageEvidence {
            locator: format!("{}@{attribute}", self.locator(element)),
            excerpt: excerpt(&normalize(element.attr(attribute).unwrap_or_default())),
        }
    }

    fn locator(&self, element: ElementRef<'_>) -> String {
        let mut path: Vec<_> = std::iter::once(element)
            .chain(element.ancestors().filter_map(ElementRef::wrap))
            .map(|node| {
                let position = node
                    .prev_siblings()
                    .filter(|node| node.value().is_element())
                    .count()
                    + 1;
                format!("{}:nth-child({position})", node.value().name())
            })
            .collect();
        path.reverse();
        let prefix = match self.format {
            Format::Html => "css:",
            Format::Markdown => "markdown-rendered-css:",
        };
        format!("{prefix}{}", path.join(" > "))
    }
}

pub fn text(element: ElementRef<'_>) -> String {
    let mut words = Vec::new();
    for node in element.descendants() {
        if let Some(value) = node.value().as_text()
            && !node.ancestors().filter_map(ElementRef::wrap).any(excluded)
        {
            words.extend(value.split_whitespace());
        }
    }
    words.join(" ")
}

pub fn metadata(source: &PageSource, page: &Page) -> Result<Extraction> {
    let base = markdown_base(source)?;
    let scope = content_scope(page);
    let title = page
        .select("meta[property='og:title'], meta[name='og:title']")
        .into_iter()
        .find_map(|element| attribute_value(page, element, "content"))
        .or_else(|| {
            page.select("title").into_iter().find_map(|element| {
                let value = text(element);
                (!value.is_empty()).then(|| LocatedValue {
                    value,
                    evidence: page.evidence(element),
                })
            })
        })
        .or_else(|| {
            let headings = page.select("h1");
            headings
                .iter()
                .copied()
                .filter(|element| in_scope(*element, scope))
                .chain(headings.iter().copied())
                .find_map(|element| {
                    let value = text(element);
                    (!value.is_empty()).then(|| LocatedValue {
                        value,
                        evidence: page.evidence(element),
                    })
                })
        });
    let canonical_url = page
        .select("link[rel][href]")
        .into_iter()
        .find_map(|element| {
            if !element
                .attr("rel")?
                .split_whitespace()
                .any(|rel| rel.eq_ignore_ascii_case("canonical"))
            {
                return None;
            }
            let url = resolve_url(source, &base, element.attr("href")?)?;
            Some(LocatedValue {
                value: url.into(),
                evidence: page.attribute_evidence(element, "href"),
            })
        });
    let published_at = page
        .select("meta[property='article:published_time'], meta[name='article:published_time']")
        .into_iter()
        .find_map(|element| attribute_value(page, element, "content"))
        .or_else(|| json_ld_publication(page, &base, canonical_url.as_ref()));
    let mut seen = HashSet::new();
    let links = page
        .select("a[href]")
        .into_iter()
        .filter(|element| {
            in_scope(*element, scope)
                && !element
                    .ancestors()
                    .filter_map(ElementRef::wrap)
                    .any(|ancestor| {
                        excluded(ancestor)
                            || ancestor.value().name() == "aside"
                            || (ancestor.value().name() == "header" && !in_scope(ancestor, scope))
                    })
        })
        .filter_map(|element| {
            let href = element.attr("href")?.trim();
            if href.is_empty() || href.starts_with('#') {
                return None;
            }
            let url = resolve_url(source, &base, href)?;
            if !super::is_official_url(&source.vendor, url.as_str()) || !seen.insert(url.clone()) {
                return None;
            }
            Some(OfficialLink {
                url: url.into(),
                label: excerpt(&text(element)),
                evidence: page.attribute_evidence(element, "href"),
            })
        })
        .take(40)
        .collect();
    Ok(Extraction {
        extractor_version: super::EXTRACTOR_VERSION,
        title,
        published_at,
        canonical_url,
        links,
        claims: Vec::new(),
        warnings: Vec::new(),
    })
}

fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn excerpt(value: &str) -> String {
    value.chars().take(240).collect()
}

fn excluded(element: ElementRef<'_>) -> bool {
    matches!(
        element.value().name(),
        "script" | "style" | "nav" | "footer"
    ) || matches!(element.attr("role"), Some("navigation" | "contentinfo"))
}

fn markdown_body(raw: &str) -> &str {
    let raw = raw.trim_start_matches('\u{feff}');
    let mut lines = raw.split_inclusive('\n');
    let first = lines.next().unwrap_or_default();
    if first.trim_end() != "---" {
        return raw;
    }
    let mut offset = first.len();
    for line in lines {
        offset += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return &raw[offset..];
        }
    }
    raw
}

fn attribute_value(page: &Page, element: ElementRef<'_>, attribute: &str) -> Option<LocatedValue> {
    let value = normalize(element.attr(attribute)?);
    (!value.is_empty()).then(|| LocatedValue {
        value,
        evidence: page.attribute_evidence(element, attribute),
    })
}

fn content_scope(page: &Page) -> Option<ElementRef<'_>> {
    for selector in [
        "main article, [role='main'] article",
        "main, [role='main']",
        "article",
        "body",
    ] {
        if let Some(element) = page
            .select(selector)
            .into_iter()
            .filter(|element| {
                !element
                    .ancestors()
                    .filter_map(ElementRef::wrap)
                    .any(|ancestor| {
                        excluded(ancestor) || matches!(ancestor.value().name(), "aside" | "header")
                    })
            })
            .max_by_key(|element| text(*element).len())
        {
            return Some(element);
        }
    }
    None
}

fn in_scope(element: ElementRef<'_>, scope: Option<ElementRef<'_>>) -> bool {
    scope.is_none_or(|scope| {
        element.id() == scope.id() || element.ancestors().any(|node| node.id() == scope.id())
    })
}

fn json_ld_publication(
    page: &Page,
    base: &Url,
    canonical: Option<&LocatedValue>,
) -> Option<LocatedValue> {
    let mut matched = Vec::new();
    let mut unspecified = Vec::new();
    for script in page.select("script[type='application/ld+json']") {
        let source: String = script.text().collect();
        let Ok(value) = serde_json::from_str::<Value>(&source) else {
            continue;
        };
        let mut dates = Vec::new();
        article_dates(&value, "", &mut dates);
        for (date, pointer, article) in dates {
            let identities: Vec<_> = [
                article.get("url"),
                article.get("mainEntityOfPage"),
                article.get("@id"),
            ]
            .into_iter()
            .flatten()
            .filter_map(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("@id").and_then(Value::as_str))
            })
            .collect();
            let matching = identities.iter().any(|identity| {
                same_page(base, identity, base.as_str())
                    || canonical
                        .is_some_and(|canonical| same_page(base, identity, &canonical.value))
            });
            if !identities.is_empty() && !matching {
                continue;
            }
            let date = LocatedValue {
                value: date.to_owned(),
                evidence: PageEvidence {
                    locator: format!("{}#json-pointer={pointer}", page.locator(script)),
                    excerpt: excerpt(date),
                },
            };
            if matching {
                matched.push(date);
            } else {
                unspecified.push(date);
            }
        }
    }
    let candidates = if matched.is_empty() {
        unspecified
    } else {
        matched
    };
    let first = candidates.first()?;
    candidates
        .iter()
        .all(|candidate| candidate.value == first.value)
        .then(|| first.clone())
}

fn same_page(base: &Url, identity: &str, expected: &str) -> bool {
    let (Ok(mut identity), Ok(mut expected)) = (base.join(identity), base.join(expected)) else {
        return false;
    };
    identity.set_fragment(None);
    expected.set_fragment(None);
    identity.as_str().trim_end_matches('/') == expected.as_str().trim_end_matches('/')
}

fn article_dates<'a>(
    value: &'a Value,
    pointer: &str,
    dates: &mut Vec<(&'a str, String, &'a Value)>,
) {
    match value {
        Value::Object(object) => {
            let is_article = |value: &Value| {
                value.as_str().is_some_and(|kind| {
                    matches!(
                        kind.rsplit(['/', '#']).next(),
                        Some("Article" | "NewsArticle" | "BlogPosting")
                    )
                })
            };
            let article = object.get("@type").is_some_and(|kind| {
                is_article(kind)
                    || kind
                        .as_array()
                        .is_some_and(|kinds| kinds.iter().any(is_article))
            });
            if article
                && let Some(date) = object.get("datePublished").and_then(Value::as_str)
                && !date.trim().is_empty()
            {
                dates.push((date, format!("{pointer}/datePublished"), value));
            }
            for (key, value) in object {
                let key = key.replace('~', "~0").replace('/', "~1");
                article_dates(value, &format!("{pointer}/{key}"), dates);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                article_dates(value, &format!("{pointer}/{index}"), dates);
            }
        }
        _ => {}
    }
}

fn markdown_base(source: &PageSource) -> Result<Url> {
    let mut url = Url::parse(&source.url).context("invalid document source URL")?;
    if source.format != Format::Markdown {
        return Ok(url);
    }
    let mut segments: Vec<_> = url.path().split('/').map(str::to_owned).collect();
    match url.host_str() {
        Some("raw.githubusercontent.com") if segments.len() >= 5 => {
            url.set_host(Some("github.com"))?;
            if segments.get(3).is_some_and(|segment| segment == "refs")
                && matches!(segments.get(4).map(String::as_str), Some("heads" | "tags"))
                && segments.len() >= 7
            {
                segments.drain(3..5);
            }
            segments.insert(3, "blob".to_owned());
        }
        Some("github.com" | "huggingface.co") => {
            let index = if matches!(
                segments.get(1).map(String::as_str),
                Some("datasets" | "spaces")
            ) {
                4
            } else {
                3
            };
            if segments
                .get(index)
                .is_some_and(|segment| segment == "raw" || segment == "resolve")
            {
                segments[index] = "blob".to_owned();
            }
        }
        _ => return Ok(url),
    }
    url.set_path(&segments.join("/"));
    Ok(url)
}

fn resolve_url(source: &PageSource, base: &Url, href: &str) -> Option<Url> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    let url = if source.format == Format::Markdown
        && base.host_str() == Some("github.com")
        && href.starts_with('/')
        && !href.starts_with("//")
    {
        let segments: Vec<_> = base.path().split('/').collect();
        if segments.get(3) == Some(&"blob") && segments.len() >= 6 {
            base.join(&format!(
                "{}/{}",
                segments[..5].join("/"),
                href.trim_start_matches('/')
            ))
            .ok()?
        } else {
            base.join(href).ok()?
        }
    } else {
        base.join(href).ok()?
    };
    matches!(url.scheme(), "http" | "https").then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::DocumentKind;

    fn source(format: Format, url: &str) -> PageSource {
        PageSource {
            id: "test".into(),
            vendor: "openai".into(),
            kind: DocumentKind::ReleaseBlog,
            url: url.into(),
            format,
            records: Vec::new(),
        }
    }

    #[test]
    fn metadata_scopes_links_and_preserves_attribute_evidence() {
        let source = source(Format::Html, "https://openai.com/index/model/");
        let page = Page::parse(&source, br#"<html><head>
            <title>Fallback title</title><meta property="og:title" content=" Model &amp; API ">
            <meta property="article:published_time" content="2025-01-02">
            <meta property="article:modified_time" content="2026-01-02">
            <link rel="canonical" href="/index/model">
            </head><body><nav><a href="/pricing">Pricing</a></nav>
            <main><article><h1>Model</h1><p>Read <a href="../../docs/model">the docs</a>.</p>
            <a href="../../docs/model">Duplicate</a><a href="https://evil.example/model">External</a>
            <nav><a href="/unrelated">Navigation</a></nav>
            <footer><a href="/footer">Footer</a></footer></article></main>
            <a href="/outside">Outside</a></body></html>"#).unwrap();
        let result = metadata(&source, &page).unwrap();
        assert_eq!(result.title.unwrap().value, "Model & API");
        assert_eq!(result.published_at.unwrap().value, "2025-01-02");
        assert_eq!(
            result.canonical_url.unwrap().value,
            "https://openai.com/index/model"
        );
        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].url, "https://openai.com/docs/model");
        assert_eq!(result.links[0].evidence.excerpt, "../../docs/model");
        let path = result.links[0]
            .evidence
            .locator
            .strip_prefix("css:")
            .unwrap()
            .strip_suffix("@href")
            .unwrap();
        assert_eq!(page.select(path).len(), 1);
        assert_eq!(page.select(path)[0].attr("href"), Some("../../docs/model"));
        assert!(result.claims.is_empty() && result.warnings.is_empty());
    }

    #[test]
    fn main_content_wins_over_sidebar_articles_and_keeps_its_header_links() {
        let source = source(Format::Html, "https://openai.com/index/model/");
        let page = Page::parse(&source, br#"<header><a href="/pricing">Site header</a></header>
            <main><header><h1>Model</h1><a href="/model-card">Model card</a></header>
            <a href="/docs/model">Docs</a>
            <aside><article>Related <a href="/other">Other model</a></article></aside></main>
            <aside><article>Another related story <a href="/unrelated">Unrelated</a></article></aside>"#).unwrap();
        let result = metadata(&source, &page).unwrap();
        assert_eq!(result.title.unwrap().value, "Model");
        assert_eq!(
            result
                .links
                .iter()
                .map(|link| link.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://openai.com/model-card",
                "https://openai.com/docs/model"
            ]
        );
    }

    #[test]
    fn publication_uses_article_json_ld_and_never_modification() {
        let source = source(Format::Html, "https://openai.com/index/model/");
        let page = Page::parse(&source, br#"<meta property="article:modified_time" content="2026-01-02">
            <time datetime="2026-01-02">Updated today</time>
            <script type="application/ld+json">{"@type":"WebPage","datePublished":"wrong"}</script>
            <script type="application/ld+json">{"@graph":[{"@type":["Article","NewsArticle"],"dateModified":"2026-01-02","datePublished":"2025-01-02"}]}</script>"#).unwrap();
        let value = metadata(&source, &page).unwrap().published_at.unwrap();
        assert_eq!(value.value, "2025-01-02");
        assert!(
            value
                .evidence
                .locator
                .ends_with("#json-pointer=/@graph/0/datePublished")
        );
        let page = Page::parse(&source, br#"<meta property="article:modified_time" content="2026-01-02">
            <script type="application/ld+json">{"@type":"BlogPosting","dateModified":"2026-01-02"}</script>"#).unwrap();
        assert!(metadata(&source, &page).unwrap().published_at.is_none());
    }

    #[test]
    fn publication_matches_the_current_article_and_rejects_ambiguous_dates() {
        let source = source(Format::Html, "https://openai.com/index/model/");
        let page = Page::parse(&source, br#"<script type="application/ld+json">invalid JSON</script>
            <script type="application/ld+json">{"@graph":[
            {"@type":"Article","url":"/index/other","datePublished":"2024-01-02"},
            {"@type":"Article","mainEntityOfPage":{"@id":"https://openai.com/index/model#article"},"datePublished":"2025-01-02"}]}</script>"#).unwrap();
        let publication = metadata(&source, &page).unwrap().published_at.unwrap();
        assert_eq!(publication.value, "2025-01-02");
        assert!(
            publication
                .evidence
                .locator
                .ends_with("#json-pointer=/@graph/1/datePublished")
        );
        let page = Page::parse(
            &source,
            br#"<script type="application/ld+json">[
            {"@type":"Article","datePublished":"2024-01-02"},
            {"@type":"Article","datePublished":"2025-01-02"}]</script>"#,
        )
        .unwrap();
        assert!(metadata(&source, &page).unwrap().published_at.is_none());
    }

    #[test]
    fn markdown_tables_embedded_html_and_evidence_replay() {
        let source = source(
            Format::Markdown,
            "https://raw.githubusercontent.com/openai/models/main/cards/README.md",
        );
        let raw = b"---\nlicense: mit\n---\n# Model card\n\n| Property | Value |\n| --- | --- |\n| Context | 128,000 |\n\n<p>HTML <strong>content</strong>.</p>\n\n[Guide](../guide.md)\n";
        let page = Page::parse(&source, raw).unwrap();
        assert_eq!(page.raw.as_bytes(), raw);
        assert!(!text(page.html.root_element()).contains("license"));
        let row = page.select("tbody tr")[0];
        assert_eq!(text(row), "Context 128,000");
        let evidence = page.evidence(row);
        let replay = Page::parse(&source, raw).unwrap();
        let selector = evidence
            .locator
            .strip_prefix("markdown-rendered-css:")
            .unwrap();
        assert_eq!(replay.select(selector).len(), 1);
        assert_eq!(replay.evidence(replay.select(selector)[0]), evidence);
        assert!(
            page.select("p")
                .iter()
                .any(|element| text(*element) == "HTML content .")
        );
        assert_eq!(
            metadata(&source, &page).unwrap().title.unwrap().value,
            "Model card"
        );
        let base = markdown_base(&source).unwrap();
        assert_eq!(
            resolve_url(&source, &base, "../guide.md").unwrap().as_str(),
            "https://github.com/openai/models/blob/main/guide.md"
        );
        assert_eq!(
            resolve_url(&source, &base, "/guide.md#intro")
                .unwrap()
                .as_str(),
            "https://github.com/openai/models/blob/main/guide.md#intro"
        );
    }

    #[test]
    fn json_ld_article_id_must_identify_the_current_page() {
        let source = source(Format::Html, "https://openai.com/index/model/");
        for (id, expected) in [
            ("https://openai.com/index/other-model/#article", false),
            ("https://openai.com/index/model/#article", true),
        ] {
            let raw = format!(
                r#"<script type="application/ld+json">{{"@type":"Article","@id":"{id}","datePublished":"2020-01-01"}}</script>"#
            );
            let page = Page::parse(&source, raw.as_bytes()).unwrap();
            assert_eq!(
                metadata(&source, &page).unwrap().published_at.is_some(),
                expected
            );
        }
    }

    #[test]
    fn markdown_urls_preserve_repository_paths() {
        for (url, href, expected) in [
            (
                "https://raw.githubusercontent.com/openai/models/refs/heads/main/cards/README.md",
                "/guide.md",
                "https://github.com/openai/models/blob/main/guide.md",
            ),
            (
                "https://huggingface.co/openai/model/raw/main/docs/README.md",
                "../guide.md#usage",
                "https://huggingface.co/openai/model/blob/main/guide.md#usage",
            ),
            (
                "https://huggingface.co/datasets/openai/models/raw/main/README.md",
                "./guide.md",
                "https://huggingface.co/datasets/openai/models/blob/main/guide.md",
            ),
        ] {
            let source = source(Format::Markdown, url);
            let base = markdown_base(&source).unwrap();
            assert_eq!(
                resolve_url(&source, &base, href).unwrap().as_str(),
                expected
            );
        }
    }

    #[test]
    fn evidence_skips_non_content_and_caps_unicode_by_character() {
        let source = source(Format::Html, "https://openai.com/index/model/");
        let page = Page::parse(&source, "<article><p>Model <b>128K</b> tokens</p><script>bad</script><style>bad</style><nav>bad</nav><footer>bad</footer></article>".as_bytes()).unwrap();
        assert_eq!(text(page.select("article")[0]), "Model 128K tokens");
        let page = Page::parse(&source, format!("<p>{}</p>", "模".repeat(300)).as_bytes()).unwrap();
        assert_eq!(
            page.evidence(page.select("p")[0]).excerpt.chars().count(),
            240
        );
    }
}

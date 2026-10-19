use anyhow::Result;
use scraper::{ElementRef, Selector};
use serde_json::{Value, json};

use super::page::{Page, text};
use super::{Claim, ClaimScope, Extraction, PageSource, Precision};

pub fn extract(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    match source.id.as_str() {
        "openai-gpt41-model" => openai(source, page, out),
        "google-gemini25-model" => google(source, page, out),
        "amazon-nova-doc" => amazon(source, page, out),
        "amazon-nova-launch" => amazon_launch(source, page, out),
        "meta-llama33-card" => meta(source, page, out),
        "xai-code-model" => {
            let headings = page.select("main h1");
            if headings.len() != 1 || text(headings[0]) != "Grok Code Fast 1" {
                out.warnings.push("The model page heading no longer identifies Grok Code Fast 1; documented aliases do not establish historical model capabilities.".into());
            }
        }
        "openai-gpt41-blog"
        | "anthropic-claude4-blog"
        | "anthropic-claude4-card"
        | "google-gemini25-blog"
        | "meta-llama33-blog"
        | "xai-code-launch" => {}
        _ => {}
    }
    Ok(())
}

pub(super) fn select<'a>(node: ElementRef<'a>, css: &str) -> Vec<ElementRef<'a>> {
    node.select(&Selector::parse(css).expect("static selector"))
        .collect()
}

pub(super) fn cells(row: ElementRef<'_>) -> Vec<ElementRef<'_>> {
    row.children()
        .filter_map(ElementRef::wrap)
        .filter(|n| matches!(n.value().name(), "th" | "td"))
        .collect()
}

pub(super) fn unique<'a>(nodes: Vec<ElementRef<'a>>) -> Option<ElementRef<'a>> {
    (nodes.len() == 1).then(|| nodes[0])
}

#[allow(clippy::too_many_arguments)]
fn emit(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    node: ElementRef<'_>,
    property: &str,
    value: Value,
    unit: Option<&str>,
    precision: Precision,
    conditions: &[&str],
) {
    for record in &source.records {
        out.claims.push(Claim {
            record: record.clone(),
            property: property.into(),
            value: value.clone(),
            unit: unit.map(str::to_owned),
            precision,
            scope: if source.id.starts_with("google-") || source.id.starts_with("amazon-") {
                ClaimScope::Interface {
                    id: source.url.clone(),
                }
            } else {
                ClaimScope::Model
            },
            conditions: conditions.iter().map(|s| (*s).to_owned()).collect(),
            evidence: page.evidence(node),
        });
    }
}

// Abbreviations preserve their advertised scale rather than pretending to be exact counts.
pub(super) fn count(raw: &str) -> Option<(u64, Precision)> {
    let raw = raw.trim();
    let (number, multiplier, precision) = match raw.as_bytes().last()? {
        b'k' | b'K' => (&raw[..raw.len() - 1], 1_000_u128, Precision::Approximate),
        b'm' | b'M' => (&raw[..raw.len() - 1], 1_000_000, Precision::Approximate),
        b'b' | b'B' => (&raw[..raw.len() - 1], 1_000_000_000, Precision::Approximate),
        _ => (raw, 1, Precision::Exact),
    };
    let mut parts = number.split('.');
    let whole = parts.next()?;
    let fraction = parts.next();
    if parts.next().is_some() {
        return None;
    }
    let groups: Vec<_> = whole.split(',').collect();
    if groups.len() > 1
        && (groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|s| s.len() != 3))
    {
        return None;
    }
    let whole = groups.concat();
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value = whole.parse::<u128>().ok()?.checked_mul(multiplier)?;
    if let Some(fraction) = fraction {
        if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let divisor = 10_u128.checked_pow(fraction.len().try_into().ok()?)?;
        let scaled = fraction.parse::<u128>().ok()?.checked_mul(multiplier)?;
        if !scaled.is_multiple_of(divisor) {
            return None;
        }
        value = value.checked_add(scaled / divisor)?;
    }
    Some((value.try_into().ok()?, precision))
}

fn number(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    node: ElementRef<'_>,
    property: &str,
    raw: &str,
    unit: &str,
) {
    if let Some((value, precision)) = count(raw) {
        emit(
            source,
            page,
            out,
            node,
            property,
            json!(value),
            Some(unit),
            precision,
            &[],
        );
    } else {
        out.warnings
            .push(format!("Unrecognized {property} value: {raw}"));
    }
}

pub(super) fn date(raw: &str) -> Option<String> {
    let normalized = raw.replace(',', "");
    let parts: Vec<_> = normalized.split_whitespace().collect();
    let month = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
    .iter()
    .position(|m| parts.first().is_some_and(|p| *p == *m || *p == &m[..3]))?
        + 1;
    if parts.len() == 2 {
        let year: i32 = parts[1].parse().ok()?;
        return Some(format!("{year:04}-{month:02}"));
    }
    if parts.len() != 3 {
        return None;
    }
    let day: u8 = parts[1].parse().ok()?;
    let year: i32 = parts[2].parse().ok()?;
    time::Date::from_calendar_date(year, time::Month::try_from(month as u8).ok()?, day).ok()?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn openai(source: &PageSource, page: &Page, out: &mut Extraction) {
    let headings = page.select("main h1, main div.text-2xl.font-semibold.whitespace-nowrap");
    if headings.len() != 1 || text(headings[0]) != "GPT-4.1" {
        out.warnings
            .push("Missing or ambiguous GPT-4.1 model heading; skipped model facts.".into());
        return;
    }
    for (suffix, property) in [
        (" context window", "context_window"),
        (" max output tokens", "max_output_tokens"),
    ] {
        let matches = page
            .select("main div")
            .into_iter()
            .filter(|n| {
                !n.children().any(|c| ElementRef::wrap(c).is_some()) && text(*n).ends_with(suffix)
            })
            .collect();
        if let Some(node) = unique(matches) {
            let value = text(node);
            number(
                source,
                page,
                out,
                node,
                property,
                value.strip_suffix(suffix).unwrap(),
                "tokens",
            );
        } else {
            out.warnings
                .push(format!("Missing or ambiguous GPT-4.1 {property} label."));
        }
    }
    let cutoffs = page
        .select("main div")
        .into_iter()
        .filter(|n| {
            !n.children().any(|c| ElementRef::wrap(c).is_some())
                && text(*n).ends_with(" knowledge cutoff")
        })
        .collect();
    if let Some(node) = unique(cutoffs)
        && let Some(value) = date(text(node).trim_end_matches(" knowledge cutoff"))
    {
        emit(
            source,
            page,
            out,
            node,
            "knowledge_cutoff",
            json!(value),
            None,
            Precision::Exact,
            &[],
        );
    }
    let labels = page
        .select("main div")
        .into_iter()
        .filter(|n| text(*n) == "Modalities")
        .collect();
    if let Some(label) = unique(labels)
        && let Some(section) = label.parent().and_then(ElementRef::wrap)
    {
        let rows: Vec<_> = select(section, "div")
            .into_iter()
            .filter_map(|n| {
                let children: Vec<_> = n.children().filter_map(ElementRef::wrap).collect();
                (children.len() == 2
                    && !children
                        .iter()
                        .any(|c| c.children().any(|n| ElementRef::wrap(n).is_some())))
                .then(|| (text(children[0]), text(children[1])))
            })
            .collect();
        modalities(source, page, out, section, rows, &[]);
    }
}

fn modalities(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    evidence: ElementRef<'_>,
    rows: Vec<(String, String)>,
    conditions: &[&str],
) {
    let mut input = Vec::new();
    let mut output = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (name, direction) in rows {
        let name = name.to_lowercase();
        if !matches!(name.as_str(), "text" | "image" | "audio" | "video") {
            continue;
        }
        if !seen.insert(name.clone()) {
            out.warnings
                .push("Duplicate modality rows; skipped modality claims.".into());
            return;
        }
        match direction.as_str() {
            "Input and output" => {
                input.push(name.clone());
                output.push(name);
            }
            "Input only" => input.push(name),
            "Output only" => output.push(name),
            "Not supported" => {}
            _ => {
                out.warnings
                    .push("Unrecognized modality direction; skipped modality claims.".into());
                return;
            }
        }
    }
    for (property, value) in [("input_modalities", input), ("output_modalities", output)] {
        if !value.is_empty() {
            emit(
                source,
                page,
                out,
                evidence,
                property,
                json!(value),
                None,
                Precision::Exact,
                conditions,
            );
        }
    }
}

fn google(source: &PageSource, page: &Page, out: &mut Extraction) {
    let candidates = page
        .select("table.geap-model-table")
        .into_iter()
        .filter(|table| {
            let ids = select(*table, "tr")
                .into_iter()
                .filter_map(|r| {
                    let c = cells(r);
                    (c.len() == 2 && text(c[0]) == "Model ID").then(|| c[1])
                })
                .collect();
            unique(ids).is_some_and(|id| text(id) == "gemini-2.5-pro")
        })
        .collect();
    let Some(table) = unique(candidates) else {
        out.warnings
            .push("Missing or ambiguous Gemini 2.5 Pro model table; skipped model facts.".into());
        return;
    };
    for (label, property) in [
        ("Context window", "context_window"),
        ("Maximum output tokens", "max_output_tokens"),
    ] {
        let values = select(table, "tr")
            .into_iter()
            .filter_map(|r| {
                let c = cells(r);
                (c.len() >= 2 && text(c[c.len() - 2]) == label).then(|| c[c.len() - 1])
            })
            .collect();
        if let Some(node) = unique(values) {
            number(source, page, out, node, property, &text(node), "tokens");
        } else {
            out.warnings
                .push(format!("Missing or ambiguous Gemini 2.5 Pro {label} row."));
        }
    }
    let sections = select(table, "tr")
        .into_iter()
        .filter(|r| cells(*r).first().is_some_and(|c| text(*c) == "Modalities"))
        .collect();
    if let Some(section) = unique(sections) {
        let rows = select(section, ".geap-modality")
            .into_iter()
            .filter_map(|n| {
                Some((
                    text(unique(select(n, ".geap-modality-label"))?),
                    text(unique(select(n, ".geap-supported-modality"))?),
                ))
            })
            .collect();
        modalities(
            source,
            page,
            out,
            section,
            rows,
            &["Google Cloud model reference"],
        );
    }
    let versions = select(table, "tr")
        .into_iter()
        .filter(|r| {
            let c = cells(*r);
            c.first().is_some_and(|n| text(*n) == "Versions")
                && select(*r, "code")
                    .iter()
                    .any(|n| text(*n) == "gemini-2.5-pro")
        })
        .collect();
    if let Some(version) = unique(versions) {
        let releases = select(version, "li")
            .into_iter()
            .filter(|n| text(*n).starts_with("Release date:"))
            .collect();
        if let Some(node) = unique(releases)
            && let Some(value) = date(text(node).trim_start_matches("Release date:").trim())
        {
            emit(
                source,
                page,
                out,
                node,
                "release_date",
                json!(value),
                None,
                Precision::Exact,
                &["Google Cloud GA release"],
            );
        }
    }
}

fn amazon(source: &PageSource, page: &Page, out: &mut Extraction) {
    let mut candidates = Vec::new();
    for table in page.select("#main-col-body table") {
        let headers: Vec<_> = select(table, "tr")
            .into_iter()
            .filter(|r| cells(*r).iter().any(|c| c.value().name() == "th"))
            .collect();
        let Some(header) = unique(headers) else {
            continue;
        };
        let header = cells(header);
        let columns: Vec<_> = header
            .iter()
            .enumerate()
            .filter(|(_, c)| text(**c) == "Amazon Nova Pro")
            .map(|(i, _)| i)
            .collect();
        if columns.len() != 1 {
            continue;
        }
        let column = columns[0];
        let rows = select(table, "tr");
        let ids: Vec<_> = rows
            .iter()
            .filter_map(|r| {
                let c = cells(*r);
                (c.len() == header.len() && text(c[0]) == "Model ID").then(|| c[column])
            })
            .collect();
        if ids.len() == 1 && text(ids[0]) == "amazon.nova-pro-v1:0" {
            candidates.push((table, column, header.len()));
        }
    }
    if candidates.len() != 1 {
        out.warnings.push(
            "Missing or ambiguous Amazon Nova Pro column and model ID; skipped model facts.".into(),
        );
        return;
    }
    let (table, column, width) = candidates[0];
    for (label, property) in [
        ("Context Window", "context_window"),
        ("Max Output Tokens", "max_output_tokens"),
        ("Input modalities", "input_modalities"),
        ("Output Modalities", "output_modalities"),
    ] {
        let values = select(table, "tr")
            .into_iter()
            .filter_map(|r| {
                let c = cells(r);
                (c.len() == width && text(c[0]) == label).then(|| c[column])
            })
            .collect();
        if let Some(node) = unique(values) {
            if property.ends_with("modalities") {
                let value: Vec<_> = text(node)
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .collect();
                if value
                    .iter()
                    .all(|s| matches!(s.as_str(), "text" | "image" | "audio" | "video"))
                {
                    emit(
                        source,
                        page,
                        out,
                        node,
                        property,
                        json!(value),
                        None,
                        Precision::Exact,
                        &["Amazon Bedrock Nova Pro v1"],
                    );
                }
            } else {
                number(source, page, out, node, property, &text(node), "tokens");
            }
        } else {
            out.warnings
                .push(format!("Missing or ambiguous Amazon Nova Pro {label} row."));
        }
    }
}

pub(super) fn amazon_launch_identity(page: &Page) -> bool {
    let model_ids: std::collections::BTreeSet<_> = page
        .select("section.blog-post-content pre code")
        .into_iter()
        .flat_map(|node| {
            text(node)
                .split(['\"', '\''])
                .filter(|value| value.starts_with("amazon.nova-pro-"))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    model_ids.len() == 1 && model_ids.contains("amazon.nova-pro-v1:0")
}

fn amazon_launch(source: &PageSource, page: &Page, out: &mut Extraction) {
    if !amazon_launch_identity(page) {
        out.warnings.push(
            "Missing or ambiguous Nova Pro v1 identifier in launch article; skipped model facts."
                .into(),
        );
        return;
    }
    let paragraphs = page
        .select("section.blog-post-content p")
        .into_iter()
        .filter(|n| {
            select(*n, "b, strong")
                .first()
                .is_some_and(|label| text(*label) == "Amazon Nova Pro")
        })
        .collect();
    if let Some(node) = unique(paragraphs) {
        let content = text(node);
        if let Some((_, tail)) = content.split_once("up to ")
            && let Some(raw) = tail
                .strip_suffix(" input tokens.")
                .or_else(|| tail.split_once(" input tokens").map(|(v, _)| v))
            && let Some((value, precision)) = count(raw)
        {
            emit(
                source,
                page,
                out,
                node,
                "context_window",
                json!(value),
                Some("tokens"),
                precision,
                &["Launch article describes the maximum input token count"],
            );
        }
    }
}

fn meta(source: &PageSource, page: &Page, out: &mut Extraction) {
    let mut matches = Vec::new();
    for table in page.select("table") {
        let headers = select(table, "thead tr");
        let Some(header) = unique(headers) else {
            continue;
        };
        let headers: Vec<_> = cells(header).iter().map(|n| text(*n)).collect();
        if !headers.iter().any(|s| s == "Params") {
            continue;
        }
        for row in select(table, "tbody tr") {
            let row = cells(row);
            if row.len() == headers.len() && text(row[0]) == "Llama 3.3 (text only)" {
                matches.push((headers.clone(), row));
            }
        }
    }
    if matches.len() != 1 {
        out.warnings.push(
            "Missing or ambiguous Llama 3.3 model information row; skipped model facts.".into(),
        );
        return;
    }
    let (headers, row) = &matches[0];
    for (label, property, unit) in [
        ("Params", "parameter_count", "parameters"),
        ("Context length", "context_window", "tokens"),
    ] {
        let columns: Vec<_> = headers
            .iter()
            .enumerate()
            .filter(|(_, s)| *s == label)
            .map(|(i, _)| i)
            .collect();
        if columns.len() == 1 {
            number(
                source,
                page,
                out,
                row[columns[0]],
                property,
                &text(row[columns[0]]),
                unit,
            );
        }
    }
    for (label, property) in [
        ("Input modalities", "input_modalities"),
        ("Output modalities", "output_modalities"),
        ("Knowledge cutoff", "knowledge_cutoff"),
    ] {
        let columns: Vec<_> = headers
            .iter()
            .enumerate()
            .filter(|(_, s)| *s == label)
            .map(|(i, _)| i)
            .collect();
        if columns.len() != 1 {
            continue;
        }
        let node = row[columns[0]];
        let value = match (property, text(node).as_str()) {
            ("knowledge_cutoff", value) => date(value).map(|s| json!(s)),
            ("input_modalities", "Multilingual Text")
            | ("output_modalities", "Multilingual Text and code") => Some(json!(["text"])),
            _ => None,
        };
        if let Some(value) = value {
            emit(
                source,
                page,
                out,
                node,
                property,
                value,
                None,
                Precision::Exact,
                &[],
            );
        }
    }
    let release_labels = page
        .select("p")
        .into_iter()
        .filter(|n| text(*n) == "Model Release Date:")
        .collect();
    if let Some(label) = unique(release_labels)
        && let Some(list) = label
            .next_siblings()
            .filter_map(ElementRef::wrap)
            .next()
            .filter(|n| n.value().name() == "ul")
    {
        let entries = select(list, "li")
            .into_iter()
            .filter(|n| text(*n).starts_with("70B Instruct:"))
            .collect();
        if let Some(node) = unique(entries)
            && let Some(value) = date(text(node).trim_start_matches("70B Instruct:").trim())
        {
            emit(
                source,
                page,
                out,
                node,
                "release_date",
                json!(value),
                None,
                Precision::Exact,
                &[],
            );
        }
    }
    let licenses = page
        .select("p")
        .into_iter()
        .filter(|n| text(*n).starts_with("License "))
        .collect();
    if let Some(node) = unique(licenses)
        && let Some((_, tail)) = text(node).split_once("the ")
        && let Some((name, _)) = tail.split_once(", is available at:")
    {
        emit(
            source,
            page,
            out,
            node,
            "license_name",
            json!(name),
            None,
            Precision::Exact,
            &[],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{DocumentKind, Format};

    fn run(id: &str, format: Format, raw: &str) -> Extraction {
        let source = PageSource {
            repository: None,
            id: id.into(),
            vendor: "test".into(),
            kind: DocumentKind::ModelPage,
            url: "https://example.com/model".into(),
            format,
            records: vec!["test:model".into()],
        };
        let page = Page::parse(&source, raw.as_bytes()).unwrap();
        let mut out = Extraction {
            extractor_version: 1,
            title: None,
            published_at: None,
            canonical_url: None,
            links: Vec::new(),
            catalog_entries: Vec::new(),
            passages: Vec::new(),
            claims: Vec::new(),
            warnings: Vec::new(),
        };
        extract(&source, &page, &mut out).unwrap();
        out
    }

    #[test]
    fn amazon_resolves_header_and_id_not_column_position() {
        let raw = r#"<div id="main-col-body"><table><tr><th></th><th>Amazon Nova Lite</th><th>Amazon Nova Pro</th></tr>
            <tr><td>Model ID</td><td>amazon.nova-lite-v1:0</td><td>amazon.nova-pro-v1:0</td></tr>
            <tr><td>Context Window</td><td>999k</td><td>300k</td></tr></table></div>"#;
        let out = run("amazon-nova-doc", Format::Html, raw);
        assert_eq!(out.claims[0].value, json!(300000));
        assert_eq!(out.claims[0].precision, Precision::Approximate);
        assert!(
            run(
                "amazon-nova-doc",
                Format::Html,
                &raw.replace("Amazon Nova Lite", "Amazon Nova Pro")
            )
            .claims
            .is_empty()
        );
        assert!(
            run(
                "amazon-nova-doc",
                Format::Html,
                &raw.replace("amazon.nova-pro-v1:0", "amazon.nova-pro-v2:0")
            )
            .claims
            .is_empty()
        );
    }

    #[test]
    fn google_requires_exact_model_and_unique_limits() {
        let raw = r#"<table class="geap-model-table"><tr><th>Model ID</th><td>gemini-2.5-pro</td></tr>
            <tr><th rowspan="2">Token limits</th><th>Context window</th><td>1,048,576</td></tr>
            <tr><th>Maximum output tokens</th><td>65,536</td></tr></table>"#;
        let out = run("google-gemini25-model", Format::Html, raw);
        assert_eq!(
            out.claims
                .iter()
                .map(|c| c.value.clone())
                .collect::<Vec<_>>(),
            vec![json!(1048576), json!(65536)]
        );
        assert!(
            run(
                "google-gemini25-model",
                Format::Html,
                &raw.replace("gemini-2.5-pro", "gemini-2.5-pro-preview")
            )
            .claims
            .is_empty()
        );
        assert!(
            run(
                "google-gemini25-model",
                Format::Html,
                &format!("{raw}{raw}")
            )
            .claims
            .is_empty()
        );
    }

    #[test]
    fn amazon_launch_requires_the_documented_v1_identifier() {
        let raw = r#"<section class="blog-post-content"><p><b>Amazon Nova Pro</b> supports up to 300K input tokens.</p>
            <pre><code>MODEL_ID = "amazon.nova-pro-v1:0"</code></pre></section>"#;
        assert_eq!(
            run("amazon-nova-launch", Format::Html, raw).claims[0].value,
            json!(300000)
        );
        assert!(
            run(
                "amazon-nova-launch",
                Format::Html,
                &raw.replace("amazon.nova-pro-v1:0", "amazon.nova-pro-v2:0")
            )
            .claims
            .is_empty()
        );
    }

    #[test]
    fn openai_limits_are_scoped_and_ambiguous_labels_fail() {
        let raw = r#"<nav><div>999 context window</div></nav><main><div class="text-2xl font-semibold whitespace-nowrap">GPT-4.1</div>
            <div>1,047,576 context window</div><div>32,768 max output tokens</div></main>"#;
        let out = run("openai-gpt41-model", Format::Html, raw);
        assert_eq!(out.claims[0].value, json!(1047576));
        let duplicate = raw.replace("</main>", "<div>999 context window</div></main>");
        assert!(
            !run("openai-gpt41-model", Format::Html, &duplicate)
                .claims
                .iter()
                .any(|c| c.property == "context_window")
        );
    }

    #[test]
    fn xai_drift_and_publication_are_not_historical_facts() {
        let out = run(
            "xai-code-model",
            Format::Html,
            "<title>Grok Code Fast 1</title><main><h1>Grok Build 0.1</h1><p>Aliases: grok-code-fast-1</p><p>256,000 context</p></main>",
        );
        assert!(out.claims.is_empty());
        assert!(!out.warnings.is_empty());
        assert!(run("anthropic-claude4-blog", Format::Html,
            "<meta property='article:published_time' content='2025-05-22'><p>Claude Sonnet 4</p>").claims.is_empty());
    }

    #[test]
    fn markdown_row_preserves_approximation_and_release_scope() {
        let raw = "## Model Information\n\n| | Params | Context length | Knowledge cutoff |\n|---|---|---|---|\n| Other | 1B | 99k | June 2020 |\n| Llama 3.3 (text only) | 70B | 128k | December 2023 |\n\n**Model Release Date:**\n\n* **70B Instruct: December 6, 2024**\n";
        let out = run("meta-llama33-card", Format::Markdown, raw);
        assert!(out.claims.iter().any(|c| c.property == "parameter_count"
            && c.value == json!(70000000000_u64)
            && c.precision == Precision::Approximate));
        assert!(
            out.claims
                .iter()
                .any(|c| c.property == "release_date" && c.value == json!("2024-12-06"))
        );
        assert!(!out.claims.iter().any(|c| c.value == json!(99000)));
    }
}

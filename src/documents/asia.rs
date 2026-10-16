use anyhow::{Context, Result};
use scraper::{ElementRef, Selector};
use serde_json::{Value, json};

use super::Extraction;
use super::model::{Claim, ClaimScope, Format, PageEvidence, PageSource, Precision};
use super::page::{Page, text};

pub fn extract(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    match source.id.as_str() {
        "deepseek-blog" => deepseek_blog(source, page, out),
        "deepseek-card" => {
            if model_heading(page, "DeepSeek-V3.2: Efficient Reasoning & Agentic AI", out) {
                frontmatter_license(source, page, out)?;
                for node in section(page, "Chat Template") {
                    let value = text(node);
                    if value.starts_with("DeepSeek-V3.2 introduces")
                        && value.contains("format for tool calling")
                    {
                        add(source, out, "tool_call", json!(true), None, Precision::Exact,
                            &[], page.evidence(node));
                    }
                }
            }
        }
        "deepseek-updates" => deepseek_updates(source, page, out),
        "qwen-blog" => qwen_blog(source, page, out),
        "qwen-card" => qwen_card(source, page, out)?,
        "qwen-ai-blog" => out.warnings.push(
            "The Qwen URL returned an application shell; model facts require the linked official static release page.".into(),
        ),
        "nvidia-blog" => nvidia_blog(source, page, out),
        "nvidia-card" | "nvidia-page" => nvidia_card(source, page, out)?,
        "west-mistral-blog" | "west-mistral-card" | "west-cohere-blog"
        | "west-cohere-doc" | "west-microsoft-blog" | "west-microsoft-card" => {
            west_extract(source, page, out)?;
        }
        _ => {}
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add(
    source: &PageSource,
    out: &mut Extraction,
    property: &str,
    value: Value,
    unit: Option<&str>,
    precision: Precision,
    conditions: &[&str],
    evidence: PageEvidence,
) {
    for record in &source.records {
        out.claims.push(Claim {
            record: record.clone(),
            property: property.into(),
            value: value.clone(),
            unit: unit.map(str::to_owned),
            precision,
            scope: if source.id == "west-cohere-doc"
                && matches!(property, "context_window" | "max_output_tokens")
            {
                ClaimScope::Interface {
                    id: source.url.clone(),
                }
            } else {
                ClaimScope::Model
            },
            conditions: conditions.iter().map(|s| (*s).to_owned()).collect(),
            evidence: evidence.clone(),
        });
    }
}

pub(super) fn model_heading(page: &Page, expected: &str, out: &mut Extraction) -> bool {
    if page.select("h1").iter().any(|n| text(*n) == expected) {
        true
    } else {
        out.warnings.push(format!(
            "Expected model heading {expected:?} was absent; model facts were skipped."
        ));
        false
    }
}

fn heading_level(node: ElementRef<'_>) -> Option<u8> {
    match node.value().name() {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

pub(super) fn section<'a>(page: &'a Page, heading: &str) -> Vec<ElementRef<'a>> {
    let Some(start) = page
        .select("h1,h2,h3,h4,h5,h6")
        .into_iter()
        .find(|n| text(*n).trim_end_matches('#').trim() == heading)
    else {
        return Vec::new();
    };
    let level = heading_level(start).unwrap();
    start
        .next_siblings()
        .filter_map(ElementRef::wrap)
        .take_while(|n| heading_level(*n).is_none_or(|next| next > level))
        .collect()
}

pub(super) fn elements<'a>(nodes: &[ElementRef<'a>], css: &str) -> Vec<ElementRef<'a>> {
    let selector = Selector::parse(css).expect("static selector");
    nodes
        .iter()
        .flat_map(|node| {
            std::iter::once(*node)
                .filter(|n| selector.matches(n))
                .chain(node.select(&selector))
        })
        .collect()
}

pub(super) fn cells(row: ElementRef<'_>) -> Vec<ElementRef<'_>> {
    row.children()
        .filter_map(ElementRef::wrap)
        .filter(|n| matches!(n.value().name(), "th" | "td"))
        .collect()
}

pub(super) fn number(value: &str) -> Option<u64> {
    let token = value.split_whitespace().next()?;
    if !token.bytes().all(|c| c.is_ascii_digit() || c == b',') {
        return None;
    }
    if token.contains(',') {
        let mut groups = token.split(',');
        let first = groups.next()?;
        if first.is_empty() || first.len() > 3 || !groups.all(|group| group.len() == 3) {
            return None;
        }
    }
    token.replace(',', "").parse().ok()
}

pub(super) fn scaled(value: &str) -> Option<u64> {
    let value = value.trim_start();
    let end = value
        .find(|c: char| !c.is_ascii_digit() && c != ',' && c != '.')
        .unwrap_or(value.len());
    let base: f64 = value[..end].replace(',', "").parse().ok()?;
    let scale = match value[end..].trim_start().chars().next()? {
        'B' | 'b' => 1_000_000_000.0,
        'M' | 'm' => 1_000_000.0,
        'K' | 'k' => 1_000.0,
        _ => return None,
    };
    let result = base * scale;
    (result.is_finite() && result >= 0.0 && result < u64::MAX as f64)
        .then_some(result.round() as u64)
}

fn frontmatter_license(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    if page.format != Format::Markdown {
        return Ok(());
    }
    let raw = page.raw.strip_prefix('\u{feff}').unwrap_or(&page.raw);
    let mut lines = raw.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return Ok(());
    }
    let start = raw.find('\n').map_or(raw.len(), |n| n + 1);
    let mut end = start;
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        end += line.len();
    }
    anyhow::ensure!(
        closed && end - start <= 128 * 1024,
        "invalid or oversized model card front matter"
    );
    let yaml = &raw[start..end];
    let options = serde_saphyr::options! {
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        merge_keys: serde_saphyr::MergeKeyPolicy::Error,
        reject_unsupported_tags: true,
        budget: serde_saphyr::budget! {
            max_documents: 1, max_aliases: 0, max_anchors: 0,
            max_depth: 16, max_nodes: 10_000, max_total_scalar_bytes: 128 * 1024,
        },
    };
    let values: Value = serde_saphyr::from_str_with_options(yaml, options)
        .context("parse model card front matter")?;
    if let Some(license) = values.get("license").and_then(Value::as_str) {
        let excerpt = yaml.lines().find(|line| line.starts_with("license:"));
        if let Some(excerpt) = excerpt {
            let offset = raw.find(excerpt).unwrap_or(start);
            add(
                source,
                out,
                "license",
                json!(license),
                None,
                Precision::Exact,
                &[],
                PageEvidence {
                    locator: format!(
                        "markdown-bytes:{offset}..{}:frontmatter/license",
                        offset + excerpt.len()
                    ),
                    excerpt: excerpt.into(),
                },
            );
        }
    }
    Ok(())
}

fn deepseek_blog(source: &PageSource, page: &Page, out: &mut Extraction) {
    if !model_heading(page, "DeepSeek-V3.2 Release", out) {
        return;
    }
    for node in page.select("article p") {
        let value = text(node);
        if value.contains("DeepSeek-V3.2 is our first model")
            && value.contains("supports tool-use in both thinking and non-thinking modes")
        {
            for property in ["tool_call", "reasoning"] {
                add(
                    source,
                    out,
                    property,
                    json!(true),
                    None,
                    Precision::Exact,
                    &[],
                    page.evidence(node),
                );
            }
        }
    }
}

fn deepseek_updates(source: &PageSource, page: &Page, out: &mut Extraction) {
    let Some(heading) = page.select("h3#deepseek-v32").into_iter().next() else {
        return;
    };
    if text(heading).trim_end_matches('\u{200b}').trim() != "DeepSeek-V3.2" {
        return;
    }
    let Some(date) = heading
        .prev_siblings()
        .filter_map(ElementRef::wrap)
        .find(|n| n.value().name() == "h2")
    else {
        return;
    };
    let date_text = text(date);
    if let Some(value) = date_text.strip_prefix("Date: ") {
        let value = value.trim_end_matches('\u{200b}').trim();
        if value.len() == 10 && value.as_bytes()[4] == b'-' && value.as_bytes()[7] == b'-' {
            add(
                source,
                out,
                "release_date",
                json!(value),
                None,
                Precision::Exact,
                &[],
                page.evidence(date),
            );
        }
    }
}

fn qwen_blog(source: &PageSource, page: &Page, out: &mut Extraction) {
    let rows = Selector::parse("tr").unwrap();
    for table in page.select("article.post-single table") {
        let mut column = None;
        for row in table.select(&rows) {
            let cells = cells(row);
            if column.is_none() {
                column = cells.iter().position(|n| text(*n) == "Context Length");
                continue;
            }
            if cells.first().is_some_and(|n| text(*n) == "Qwen3-8B")
                && let Some(cell) = column.and_then(|i| cells.get(i))
            {
                add(
                    source,
                    out,
                    "context_window",
                    json!(text(*cell)),
                    Some("tokens"),
                    Precision::Approximate,
                    &["Release table uses abbreviated units; native context is not specified."],
                    page.evidence(*cell),
                );
            }
        }
    }
}

fn qwen_card(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    if !model_heading(page, "Qwen3-8B", out) {
        return Ok(());
    }
    frontmatter_license(source, page, out)?;
    for node in elements(&section(page, "Model Overview"), "li") {
        let value = text(node);
        if let Some(count) = value
            .strip_prefix("Number of Parameters: ")
            .and_then(scaled)
        {
            add(
                source,
                out,
                "parameter_count",
                json!(count),
                Some("parameters"),
                Precision::Approximate,
                &["Advertised rounded total parameter count."],
                page.evidence(node),
            );
        }
        if let Some(length) = value.strip_prefix("Context Length: ")
            && let Some((native, extended)) = length.split_once(" natively and ")
        {
            if let Some(count) = number(native) {
                add(
                    source,
                    out,
                    "context_window",
                    json!(count),
                    Some("tokens"),
                    Precision::Exact,
                    &["Native context."],
                    page.evidence(node),
                );
            }
            if extended.contains("tokens with YaRN")
                && let Some(count) = number(extended)
            {
                add(
                    source,
                    out,
                    "context_window",
                    json!(count),
                    Some("tokens"),
                    Precision::Exact,
                    &["YaRN context extension required."],
                    page.evidence(node),
                );
            }
        }
    }
    Ok(())
}

fn nvidia_blog(source: &PageSource, page: &Page, out: &mut Extraction) {
    let article = "article#post-90771";
    let link = page.select("article#post-90771 a").iter().any(|n| {
        n.value().attr("href") == Some("https://build.nvidia.com/nvidia/nemotron-3-super-120b-a12b")
    });
    if !link {
        out.warnings
            .push("NVIDIA release page lacked the expected model detail link.".into());
        return;
    }
    for node in page.select(&format!("{article} p")) {
        let value = text(node);
        if let Some(rest) = value.strip_prefix("Launched today, NVIDIA Nemotron 3 Super is a ") {
            if let Some((count, _)) = rest.split_once("‑billion‑parameter")
                && let Some(count) = scaled(&format!("{count}B"))
            {
                add(
                    source,
                    out,
                    "parameter_count",
                    json!(count),
                    Some("parameters"),
                    Precision::Approximate,
                    &["Advertised rounded total parameter count."],
                    page.evidence(node),
                );
            }
            if let Some((_, active)) = rest.split_once(" with ")
                && let Some((count, _)) = active.split_once(" billion active parameters")
                && let Some(count) = scaled(&format!("{count}B"))
            {
                add(
                    source,
                    out,
                    "active_parameter_count",
                    json!(count),
                    Some("parameters"),
                    Precision::Approximate,
                    &["Advertised rounded active parameter count."],
                    page.evidence(node),
                );
            }
        }
        if let Some(rest) = value.strip_prefix("Nemotron 3 Super has a ")
            && let Some((size, _)) = rest.split_once(" context window")
        {
            add(
                source,
                out,
                "context_window",
                json!(size),
                Some("tokens"),
                Precision::Approximate,
                &[
                    "Marketing abbreviation; exact token capacity is not established by this statement.",
                ],
                page.evidence(node),
            );
        }
    }
}

fn nvidia_card(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    let expected = if source.id == "nvidia-card" {
        "NVIDIA-Nemotron-3-Super-120B-A12B-BF16"
    } else {
        "NVIDIA-Nemotron-3-Super-120B-A12B"
    };
    if !model_heading(page, expected, out) {
        return Ok(());
    }
    frontmatter_license(source, page, out)?;
    for row in elements(&section(page, "Model Summary"), "tr") {
        let cols = cells(row);
        let [label, value] = cols.as_slice() else {
            continue;
        };
        let content = text(*value);
        match text(*label).as_str() {
            "Total Parameters" => {
                if let Some(count) = scaled(&content) {
                    add(
                        source,
                        out,
                        "parameter_count",
                        json!(count),
                        Some("parameters"),
                        Precision::Approximate,
                        &["Advertised rounded total parameter count."],
                        page.evidence(row),
                    );
                }
                if let Some((_, active)) = content.split_once('(')
                    && active.contains("active")
                    && let Some(count) = scaled(active)
                {
                    add(
                        source,
                        out,
                        "active_parameter_count",
                        json!(count),
                        Some("parameters"),
                        Precision::Approximate,
                        &["Advertised rounded active parameter count."],
                        page.evidence(row),
                    );
                }
            }
            "Context Length" => add(
                source,
                out,
                "context_window",
                json!(content),
                Some("tokens"),
                Precision::Approximate,
                &[
                    "Model card uses abbreviated units; serving configuration may set a lower default.",
                ],
                page.evidence(row),
            ),
            "Release Date" => {
                if let Some(date) = west_date(&content) {
                    add(
                        source,
                        out,
                        "release_date",
                        json!(date),
                        None,
                        Precision::Exact,
                        &[],
                        page.evidence(row),
                    );
                }
            }
            "License" => add(
                source,
                out,
                "license_name",
                json!(content),
                None,
                Precision::Exact,
                &[],
                page.evidence(row),
            ),
            _ => {}
        }
    }
    for (heading, label, property) in [
        ("Input", "Input Type(s): ", "input_modalities"),
        ("Output", "Output Type(s): ", "output_modalities"),
    ] {
        for node in elements(&section(page, heading), "li") {
            if let Some(value) = text(node).strip_prefix(label) {
                add(
                    source,
                    out,
                    property,
                    json!([value.to_lowercase()]),
                    None,
                    Precision::Exact,
                    &[],
                    page.evidence(node),
                );
            }
        }
    }
    Ok(())
}
pub(super) fn west_extract(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
) -> anyhow::Result<()> {
    match source.id.as_str() {
        "west-mistral-blog" => {
            if !page.select("a[href]").iter().any(|a| {
                a.value().attr("href")
                    == Some("https://huggingface.co/mistralai/Magistral-Small-2506")
            }) {
                out.warnings
                    .push("Magistral Small exact repository link missing".into());
                return Ok(());
            }
            for node in page.select("p, li") {
                let text = super::page::text(node);
                let Some((_, small)) = text.split_once("Magistral Small") else {
                    continue;
                };
                // Stop before the next variant so Medium's number cannot become Small's claim.
                let small = small.split("Magistral Medium").next().unwrap_or(small);
                if let Some(value) = west_parameters(small) {
                    add(
                        source,
                        out,
                        "parameter_count",
                        serde_json::json!(value),
                        Some("parameters"),
                        Precision::Approximate,
                        &[],
                        page.evidence(node),
                    );
                }
                if let Some((_, license)) = small.split_once("under the ")
                    && let Some((license, _)) = license.split_once(" license")
                {
                    add(
                        source,
                        out,
                        "license",
                        serde_json::json!(license.to_lowercase().replace(' ', "-")),
                        None,
                        Precision::Exact,
                        &[],
                        page.evidence(node),
                    );
                }
            }
        }
        "west-mistral-card" => {
            if !model_heading(page, "Magistral Small 1.0", out) {
                return Ok(());
            }
            frontmatter_license(source, page, out)?;
            for node in page.select("p") {
                let text = super::page::text(node);
                if text.starts_with("Building upon Mistral Small")
                    && let Some(value) = west_parameters(&text)
                {
                    add(
                        source,
                        out,
                        "parameter_count",
                        serde_json::json!(value),
                        Some("parameters"),
                        Precision::Approximate,
                        &[],
                        page.evidence(node),
                    );
                }
            }
            for node in page.select("li") {
                let text = super::page::text(node);
                if let Some(value) = text
                    .strip_prefix("Context Window:")
                    .and_then(|s| s.trim().strip_prefix("A "))
                    .and_then(scaled)
                {
                    add(
                        source,
                        out,
                        "context_window",
                        serde_json::json!(value),
                        Some("tokens"),
                        Precision::Approximate,
                        &[
                            "Advertised capacity; source separately recommends a shorter context for quality",
                        ],
                        page.evidence(node),
                    );
                }
            }
        }
        "west-cohere-doc" => {
            if !page
                .select("div")
                .iter()
                .any(|n| super::page::text(*n) == "command-a-03-2025")
            {
                out.warnings.push("Command A exact Model ID missing".into());
                return Ok(());
            }
            for label in page.select(".model-showcase-grid strong") {
                let key = super::page::text(label);
                let property = match key.as_str() {
                    "Context Window:" => "context_window",
                    "Max Output Tokens:" => "max_output_tokens",
                    _ => continue,
                };
                let Some(row) = label.parent().and_then(scraper::ElementRef::wrap) else {
                    continue;
                };
                let row_text = super::page::text(row);
                if let Some(value) = row_text.strip_prefix(&key).and_then(|s| {
                    let mut parts = s.split_whitespace();
                    let integer = parts.next()?;
                    (parts.next() == Some("tokens") && parts.next().is_none())
                        .then(|| number(integer))
                        .flatten()
                }) {
                    add(
                        source,
                        out,
                        property,
                        serde_json::json!(value),
                        Some("tokens"),
                        Precision::Exact,
                        &[],
                        page.evidence(row),
                    );
                } else {
                    out.warnings.push(format!(
                        "Command A {key} did not contain an exact integer token count; claim skipped."
                    ));
                }
            }
            for node in section(page, "Description") {
                let text = super::page::text(node);
                if text.contains("Command A")
                    && let Some(value) = west_parameters(&text)
                {
                    add(
                        source,
                        out,
                        "parameter_count",
                        serde_json::json!(value),
                        Some("parameters"),
                        Precision::Approximate,
                        &[],
                        page.evidence(node),
                    );
                }
            }
        }
        "west-cohere-blog" => {
            let bodies = west_flight_bodies(page);
            if bodies.len() != 1 {
                out.warnings.push(format!(
                    "Expected one Command A Next Flight article, found {}",
                    bodies.len()
                ));
                return Ok(());
            }
            let (record, body) = &bodies[0];
            let embedded = Page {
                html: scraper::Html::parse_document(body),
                raw: body.clone(),
                format: Format::Html,
            };
            let prefix = format!("embedded-rsc:script:self.__next_f.push:record{record}:");
            west_embedded_links(source, &embedded, out, &prefix)?;
            for node in section(&embedded, "Enterprise-ready capabilities") {
                let text = super::page::text(node);
                let Some((before, _)) = text.split_once(" context length") else {
                    continue;
                };
                let Some(value) = before.split_whitespace().last().and_then(scaled) else {
                    continue;
                };
                let mut evidence = embedded.evidence(node);
                evidence.locator = format!("{prefix}{}", evidence.locator);
                add(
                    source,
                    out,
                    "context_window",
                    serde_json::json!(value),
                    Some("tokens"),
                    Precision::Approximate,
                    &[],
                    evidence,
                );
            }
        }
        "west-microsoft-blog" => {
            const POINTER: &str =
                "/props/pageProps/apolloState/BlogTopicMessage:message:4357090/body";
            let Some(script) = page.select("script#__NEXT_DATA__").first().copied() else {
                out.warnings.push("Phi-4 article JSON missing".into());
                return Ok(());
            };
            let data: serde_json::Value = serde_json::from_str(&script.text().collect::<String>())?;
            let Some(body) = data.pointer(POINTER).and_then(serde_json::Value::as_str) else {
                out.warnings
                    .push("Phi-4 article body missing from JSON".into());
                return Ok(());
            };
            let embedded = Page {
                html: scraper::Html::parse_document(body),
                raw: body.to_owned(),
                format: Format::Html,
            };
            let prefix = format!("embedded-json:script#__NEXT_DATA__:{POINTER}:");
            west_embedded_links(source, &embedded, out, &prefix)?;
            for node in embedded.select("p") {
                let text = super::page::text(node);
                if text.starts_with("Today we are introducing")
                    && text.contains("Phi-4")
                    && let Some(value) = west_parameters(&text)
                {
                    let mut evidence = embedded.evidence(node);
                    evidence.locator = format!("{prefix}{}", evidence.locator);
                    add(
                        source,
                        out,
                        "parameter_count",
                        serde_json::json!(value),
                        Some("parameters"),
                        Precision::Approximate,
                        &[],
                        evidence,
                    );
                }
            }
        }
        "west-microsoft-card" => {
            if !model_heading(page, "Phi-4 Model Card", out) {
                return Ok(());
            }
            frontmatter_license(source, page, out)?;
            let cells = scraper::Selector::parse("th, td").expect("static selector");
            let rows = scraper::Selector::parse("tr").expect("static selector");
            for table in section(page, "Model Summary") {
                if table.value().name() != "table" {
                    continue;
                }
                for row in table.select(&rows) {
                    let columns: Vec<_> = row.select(&cells).collect();
                    if columns.len() != 2 {
                        continue;
                    }
                    let label = super::page::text(columns[0]);
                    let value = super::page::text(columns[1]);
                    let evidence = page.evidence(row);
                    match label.as_str() {
                        "Architecture" => {
                            if let Some(value) = west_parameters(&value) {
                                add(
                                    source,
                                    out,
                                    "parameter_count",
                                    serde_json::json!(value),
                                    Some("parameters"),
                                    Precision::Approximate,
                                    &[],
                                    evidence,
                                );
                            }
                        }
                        "Context length" => {
                            if let Some(value) = scaled(&value) {
                                add(
                                    source,
                                    out,
                                    "context_window",
                                    serde_json::json!(value),
                                    Some("tokens"),
                                    Precision::Approximate,
                                    &[],
                                    evidence,
                                );
                            }
                        }
                        "Inputs" if value.starts_with("Text") => add(
                            source,
                            out,
                            "input_modalities",
                            serde_json::json!(["text"]),
                            None,
                            Precision::Exact,
                            &[],
                            evidence,
                        ),
                        "Outputs" if value.starts_with("Generated text") => add(
                            source,
                            out,
                            "output_modalities",
                            serde_json::json!(["text"]),
                            None,
                            Precision::Exact,
                            &[],
                            evidence,
                        ),
                        "Release date" => {
                            if let Some(value) = west_date(&value) {
                                add(
                                    source,
                                    out,
                                    "release_date",
                                    serde_json::json!(value),
                                    None,
                                    Precision::Exact,
                                    &[],
                                    evidence,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn west_parameters(text: &str) -> Option<u64> {
    let (before, _) = text.split_once(" parameter")?;
    before.split_whitespace().last().and_then(scaled)
}

pub(super) fn west_date(text: &str) -> Option<String> {
    let mut parts = text.split_whitespace();
    let month_name = parts.next()?;
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
    .position(|m| *m == month_name)?
        + 1;
    let day: u8 = parts.next()?.trim_end_matches(',').parse().ok()?;
    let year: u16 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || day == 0 || day > 31 {
        return None;
    }
    time::Date::from_calendar_date(year.into(), time::Month::try_from(month as u8).ok()?, day)
        .ok()
        .map(|date| date.to_string())
}

fn west_embedded_links(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    prefix: &str,
) -> anyhow::Result<()> {
    for mut link in super::page::metadata(source, page)?.links {
        link.evidence.locator = format!("{prefix}{}", link.evidence.locator);
        out.links.push(link);
    }
    Ok(())
}

pub(super) fn west_flight_bodies(page: &Page) -> Vec<(String, String)> {
    let mut stream = String::new();
    for script in page.select("script") {
        let script = script.text().collect::<String>();
        let script = script.trim().trim_end_matches(';');
        let Some(array) = script
            .strip_prefix("self.__next_f.push(")
            .and_then(|s| s.strip_suffix(')'))
        else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(array) else {
            continue;
        };
        if value.get(0).and_then(serde_json::Value::as_u64) == Some(1)
            && let Some(payload) = value.get(1).and_then(serde_json::Value::as_str)
        {
            stream.push_str(payload);
        }
    }
    west_flight_text_records(&stream)
        .into_iter()
        .filter(|(_, body)| {
            body.contains("command-a-03-2025") && body.contains("Enterprise-ready capabilities")
        })
        .collect()
}

fn west_flight_text_records(stream: &str) -> Vec<(String, String)> {
    let mut records = Vec::new();
    for (start, _) in stream.match_indices(":T") {
        let id = stream[..start].rsplit('\n').next().unwrap_or("");
        if id.is_empty() || id.len() > 16 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let header = &stream[start + 2..];
        let Some(comma) = header.find(',') else {
            continue;
        };
        let Ok(length) = usize::from_str_radix(&header[..comma], 16) else {
            continue;
        };
        let body_start = start + 2 + comma + 1;
        let Some(body_end) = body_start.checked_add(length) else {
            continue;
        };
        let Some(body) = stream.get(body_start..body_end) else {
            continue;
        };
        records.push((id.to_owned(), body.to_owned()));
    }
    records
}

#[cfg(test)]
mod west_extraction_tests {
    use super::*;

    #[test]
    fn flight_lengths_are_utf8_bytes_and_reject_truncated_records() {
        assert_eq!(
            west_flight_text_records("1:T3,中\n2:T9,short"),
            vec![("1".into(), "中".into())]
        );
    }

    #[test]
    fn parameter_label_ignores_earlier_model_version_digits() {
        assert_eq!(
            west_parameters("Built upon Model 3.1 (2503), with 24B parameters."),
            Some(24_000_000_000)
        );
        assert_eq!(
            west_parameters("Context 128k tokens, not a parameter count"),
            None
        );
    }
}

#[cfg(test)]
mod extraction_tests {
    use super::super::model::DocumentKind;
    use super::*;

    fn run(id: &str, body: &str, format: Format) -> Extraction {
        let source = PageSource {
            id: id.into(),
            vendor: "test".into(),
            kind: DocumentKind::ModelCard,
            url: "https://huggingface.co/test/model/raw/revision/README.md".into(),
            format,
            records: vec!["test:model".into()],
        };
        let page = Page::parse(&source, body.as_bytes()).unwrap();
        let mut out = Extraction {
            extractor_version: 1,
            title: None,
            published_at: None,
            canonical_url: None,
            links: Vec::new(),
            passages: Vec::new(),
            claims: Vec::new(),
            warnings: Vec::new(),
        };
        extract(&source, &page, &mut out).unwrap();
        out
    }

    #[test]
    fn cohere_exact_limits_reject_abbreviations_decimals_and_malformed_groups() {
        for invalid in [
            "256k", "32,768.5", "1,00,000", "256 k", "1,,000", "1000,000",
        ] {
            let body = format!(
                "<div>command-a-03-2025</div><div class='model-showcase-grid'><p><strong>Context Window:</strong> {invalid} tokens</p><p><strong>Max Output Tokens:</strong> 8,000 tokens</p></div>"
            );
            let out = run("west-cohere-doc", &body, Format::Html);
            assert_eq!(out.claims.len(), 1, "{invalid}");
            assert_eq!(out.claims[0].property, "max_output_tokens");
            assert_eq!(out.claims[0].value, json!(8000));
            assert_eq!(out.warnings.len(), 1, "{invalid}");
        }
        assert_eq!(number("32,768 natively and more"), Some(32768));
        assert_eq!(number("131072 tokens with YaRN"), Some(131072));
    }

    #[test]
    fn deepseek_release_date_ignores_anchor_text_and_neighboring_models() {
        let out = run(
            "deepseek-updates",
            "<article><h2>Date: 2025-12-01<a>\u{200b}</a></h2><h3 id='deepseek-v32'>DeepSeek-V3.2<a>\u{200b}</a></h3><p>released</p><h2>Date: 2025-09-29</h2><h3 id='deepseek-v32-exp'>DeepSeek-V3.2-Exp</h3></article>",
            Format::Html,
        );
        assert_eq!(out.claims.len(), 1);
        assert_eq!(out.claims[0].value, json!("2025-12-01"));
    }

    #[test]
    fn qwen_native_and_yarn_contexts_keep_conditions_and_ignore_examples() {
        let out = run(
            "qwen-card",
            "---\nlicense: apache-2.0\n---\n# Qwen3-8B\n\n## Model Overview\n\n- Number of Parameters: 8.2B\n- Number of Paramaters (Non-Embedding): 6.95B\n- Context Length: 32,768 natively and [131,072 tokens with YaRN](#long-texts).\n\n## Best Practices\n\n- Context Length: 999,999 natively and 8,888 tokens with YaRN.\n",
            Format::Markdown,
        );
        let context: Vec<_> = out
            .claims
            .iter()
            .filter(|c| c.property == "context_window")
            .collect();
        assert_eq!(context.len(), 2);
        assert_eq!(context[0].value, json!(32768));
        assert_eq!(context[1].value, json!(131072));
        assert_ne!(context[0].conditions, context[1].conditions);
        let parameters = out
            .claims
            .iter()
            .find(|c| c.property == "parameter_count")
            .unwrap();
        assert_eq!(parameters.value, json!(8_200_000_000u64));
        assert_eq!(parameters.precision, Precision::Approximate);
        assert!(!out.claims.iter().any(|c| c.property == "max_output_tokens"));
    }

    #[test]
    fn qwen_blog_selects_exact_model_row_without_expanding_k_units() {
        let body = "<article class='post-single'><table><tr><th>Models</th><th>Context Length</th></tr><tr><td>Qwen3-4B</td><td>32K</td></tr><tr><td>Qwen3-8B</td><td>128K</td></tr></table></article>";
        let out = run("qwen-blog", body, Format::Html);
        assert_eq!(out.claims.len(), 1);
        assert_eq!(out.claims[0].value, json!("128K"));
        assert_eq!(out.claims[0].precision, Precision::Approximate);
    }

    #[test]
    fn nvidia_summary_keeps_rounded_counts_separate_from_other_tables() {
        let out = run(
            "nvidia-card",
            "# NVIDIA-Nemotron-3-Super-120B-A12B-BF16\n\n## Model Summary\n\n| Field | Value |\n|---|---|\n| Total Parameters | 120B (12B active) |\n| Context Length | Up to 1M tokens |\n\n## Benchmarks\n\n| Field | Value |\n|---|---|\n| Total Parameters | 999B (99B active) |\n\n## Output\n\n- **Output Type(s):** Text\n- **Other Properties Related to Output:** Maximum context length up to 1M tokens\n",
            Format::Markdown,
        );
        let parameters: Vec<_> = out
            .claims
            .iter()
            .filter(|c| c.unit.as_deref() == Some("parameters"))
            .collect();
        assert_eq!(parameters.len(), 2);
        assert_eq!(parameters[0].property, "parameter_count");
        assert_eq!(parameters[0].value, json!(120_000_000_000u64));
        assert_eq!(parameters[1].property, "active_parameter_count");
        assert_eq!(parameters[1].value, json!(12_000_000_000u64));
        assert!(
            parameters
                .iter()
                .all(|c| c.precision == Precision::Approximate)
        );
        assert!(!out.claims.iter().any(|c| c.property == "max_output_tokens"));
    }
}

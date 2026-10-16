use std::collections::BTreeSet;

use anyhow::Result;
use scraper::ElementRef;
use serde_json::{Map, Value, json};

use super::content;
use super::page::{Page, text};
use super::west::{amazon_launch_identity, cells, count, date, select, unique};
use super::{Claim, ClaimScope, Extraction, PageSource, PassageTopic, Precision};

pub fn extract(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    blog_passages(source, page, out);
    match source.id.as_str() {
        "openai-gpt41-model" => openai_details(source, page, out),
        "google-gemini25-model" => google_details(source, page, out),
        "amazon-nova-doc" => amazon_details(source, page, out),
        "meta-llama33-card" if meta_identity(page) => meta_details(source, page, out),
        "anthropic-claude4-blog" => anthropic_details(source, page, out),
        "xai-code-launch" => xai_launch_details(source, page, out),
        _ => {}
    }
    Ok(())
}

fn find<'a>(page: &'a Page, css: &str, prefix: &str) -> Option<ElementRef<'a>> {
    unique(
        page.select(css)
            .into_iter()
            .filter(|n| text(*n).starts_with(prefix))
            .collect(),
    )
}

#[allow(clippy::too_many_arguments)]
fn add(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    node: ElementRef<'_>,
    property: &str,
    value: Value,
    scope: ClaimScope,
    conditions: &[&str],
) {
    for record in &source.records {
        out.claims.push(Claim {
            record: record.clone(),
            property: property.into(),
            value: value.clone(),
            unit: None,
            precision: Precision::Exact,
            scope: scope.clone(),
            conditions: conditions.iter().map(|s| (*s).to_owned()).collect(),
            evidence: page.evidence(node),
        });
    }
}

fn interface(source: &PageSource) -> ClaimScope {
    ClaimScope::Interface {
        id: source.url.clone(),
    }
}

fn direct_text(node: ElementRef<'_>) -> String {
    node.children()
        .filter_map(|n| n.value().as_text())
        .flat_map(|s| s.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

type PassageSelection = (&'static str, &'static str, PassageTopic, &'static str, bool);

fn blog_passages(source: &PageSource, page: &Page, out: &mut Extraction) {
    use PassageTopic::{Availability, Capabilities, Limitations, Overview, Usage};
    let selections: &[PassageSelection] = match source.id.as_str() {
        "anthropic-claude4-blog" => &[
            (
                "main article p.post-text",
                "Today, we’re introducing",
                Overview,
                "Introduction",
                false,
            ),
            (
                "main article p.post-text",
                "Claude Opus 4 and Sonnet 4 are hybrid models offering",
                Availability,
                "Introduction",
                false,
            ),
            (
                "main article li",
                "Extended thinking with tool use (beta)",
                Capabilities,
                "Introduction",
                false,
            ),
            (
                "main article p.post-text",
                "Finally, we've introduced thinking summaries",
                Limitations,
                "Model improvements",
                false,
            ),
        ],
        "google-gemini25-blog" => &[
            (
                "[data-component='uni-article-body'] .rich-text p",
                "We designed Gemini 2.5",
                Overview,
                "Introduction",
                false,
            ),
            (
                "[data-component='uni-article-body'] .rich-text p",
                "Thanks to all of your feedback",
                Overview,
                "Making 2.5 Flash and 2.5 Pro generally available",
                false,
            ),
            (
                "[data-component='uni-article-body'] .rich-text p",
                "The preview of Gemini 2.5 Flash-Lite",
                Availability,
                "Introducing Gemini 2.5 Flash-Lite",
                false,
            ),
        ],
        "amazon-nova-launch" => &[
            (
                "section.blog-post-content > p",
                "Today, we’re thrilled",
                Overview,
                "Introduction",
                false,
            ),
            (
                "section.blog-post-content > p",
                "Amazon Nova Pro –",
                Capabilities,
                "Understanding models: Text and visual intelligence",
                true,
            ),
            (
                "section.blog-post-content > p",
                "I can use a more detailed prompt",
                Limitations,
                "Using Amazon Nova Pro for video analysis",
                true,
            ),
            (
                "section.blog-post-content > p",
                "Things to know",
                Availability,
                "Things to know",
                false,
            ),
        ],
        "meta-llama33-blog" => &[
            (
                "p",
                "The growth of Llama",
                Overview,
                "An incredible year for Llama",
                false,
            ),
            (
                "p",
                "Meeting the growing demand",
                Availability,
                "An incredible year for Llama",
                false,
            ),
            (
                "p",
                "As more people turn",
                Usage,
                "Growing adoption by enterprises and governments",
                false,
            ),
        ],
        "xai-code-launch" => &[
            (
                "div.text-center > p",
                "We're thrilled to introduce grok-code-fast-1",
                Overview,
                "Introduction",
                true,
            ),
            (
                "div.prose.text-primary > p",
                "Throughout the training process",
                Capabilities,
                "A speedy daily driver",
                true,
            ),
            (
                "div.prose.text-primary > p",
                "The model is generally available via the xAI API",
                Availability,
                "Grok Code for everyone",
                true,
            ),
            (
                "div.prose.text-primary > p",
                "We'll focus on delivering",
                Limitations,
                "What to expect in the next few weeks",
                false,
            ),
        ],
        _ => &[],
    };
    for (css, prefix, topic, heading, bound) in selections {
        if let Some(node) = find(page, css, prefix) {
            let bound =
                *bound && (source.id != "amazon-nova-launch" || amazon_launch_identity(page));
            content::push(
                out,
                page,
                node,
                *topic,
                Some(heading),
                if bound { &source.records } else { &[] },
            );
        }
    }
}

fn openai_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    let headings = page.select("main h1, main div.text-2xl.font-semibold.whitespace-nowrap");
    if headings.len() != 1 || text(headings[0]) != "GPT-4.1" {
        return;
    }
    if let Some(node) = find(page, "main p", "GPT-4.1 excels") {
        content::push(
            out,
            page,
            node,
            PassageTopic::Overview,
            Some("Model summary"),
            &source.records,
        );
        if text(node).contains("without a reasoning step") {
            add(
                source,
                page,
                out,
                node,
                "reasoning",
                json!(false),
                ClaimScope::Model,
                &[],
            );
        }
    }
    let sections = page
        .select("main div")
        .into_iter()
        .filter(|n| text(*n) == "Features")
        .collect();
    if let Some(section) = unique(sections)
        .and_then(|n| n.parent())
        .and_then(ElementRef::wrap)
    {
        for (label, property) in [
            ("Streaming", "streaming"),
            ("Function calling", "tool_call"),
            ("Structured outputs", "structured_output"),
            ("Fine-tuning", "fine_tuning"),
            ("Predicted outputs", "predicted_outputs"),
        ] {
            let rows = select(section, "div")
                .into_iter()
                .filter(|n| {
                    let children: Vec<_> = n.children().filter_map(ElementRef::wrap).collect();
                    children.len() == 2
                        && text(children[0]) == label
                        && matches!(text(children[1]).as_str(), "Supported" | "Not supported")
                })
                .collect();
            if let Some(node) = unique(rows) {
                let status = node.children().filter_map(ElementRef::wrap).nth(1).unwrap();
                add(
                    source,
                    page,
                    out,
                    node,
                    property,
                    json!(text(status) == "Supported"),
                    interface(source),
                    &[],
                );
            }
        }
    }
    let labels = page
        .select("main div")
        .into_iter()
        .filter(|n| text(*n) == "Snapshots")
        .collect();
    if let Some(section) = unique(labels)
        .and_then(|n| n.parent())
        .and_then(ElementRef::wrap)
    {
        let ids: BTreeSet<_> = select(section, "div")
            .into_iter()
            .map(direct_text)
            .filter(|s| {
                (s == "gpt-4.1" || s.starts_with("gpt-4.1-")) && !s.contains(char::is_whitespace)
            })
            .collect();
        if !ids.is_empty() {
            add(
                source,
                page,
                out,
                section,
                "model_versions",
                json!(ids),
                interface(source),
                &[
                    "Documented OpenAI aliases and snapshots; not an identity merge across providers",
                ],
            );
        }
    }
}

fn google_table(page: &Page) -> Option<ElementRef<'_>> {
    unique(
        page.select("table.geap-model-table")
            .into_iter()
            .filter(|table| {
                let ids = select(*table, "tr")
                    .into_iter()
                    .filter_map(|row| {
                        let c = cells(row);
                        (c.len() == 2 && text(c[0]) == "Model ID").then(|| c[1])
                    })
                    .collect();
                unique(ids).is_some_and(|n| text(n) == "gemini-2.5-pro")
            })
            .collect(),
    )
}

fn named_row<'a>(table: ElementRef<'a>, label: &str) -> Option<ElementRef<'a>> {
    unique(
        select(table, "tr")
            .into_iter()
            .filter(|r| cells(*r).first().is_some_and(|n| text(*n) == label))
            .collect(),
    )
}

fn feature_status(node: ElementRef<'_>) -> Option<bool> {
    let yes = select(node, ".geap-supported");
    let no = select(node, ".geap-not-supported");
    match (yes.as_slice(), no.as_slice()) {
        ([value], []) if text(*value) == "Supported" => Some(true),
        ([], [value]) if text(*value) == "Not supported" => Some(false),
        _ => None,
    }
}

fn feature_label(node: ElementRef<'_>) -> String {
    // Links carry some labels; status/type/preview spans are separate children.
    let label: Vec<_> = node
        .children()
        .filter_map(|n| {
            if let Some(t) = n.value().as_text() {
                Some(t.to_string())
            } else {
                ElementRef::wrap(n)
                    .filter(|e| matches!(e.value().name(), "a" | "b" | "strong"))
                    .map(text)
            }
        })
        .collect();
    label
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn google_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    let Some(table) = google_table(page) else {
        return;
    };
    for (row_name, mapping) in [
        (
            "Capabilities",
            &[
                ("Thinking", "reasoning"),
                ("System instructions", "system_instructions"),
                ("Structured output", "structured_output"),
                ("Context caching", "context_caching"),
                ("RAG Engine", "rag_engine"),
                ("Tuning", "fine_tuning"),
                ("URL context", "url_context"),
                ("Agentic video understanding", "agentic_video_understanding"),
            ][..],
        ),
        (
            "Tools",
            &[
                ("Grounding", "grounding"),
                ("Code execution", "code_execution"),
                ("Function calling", "tool_call"),
                ("Computer use", "computer_use"),
            ][..],
        ),
    ] {
        if let Some(row) = named_row(table, row_name) {
            for (label, property) in mapping {
                let matches = select(row, "li.geap-feature")
                    .into_iter()
                    .filter(|n| feature_label(*n) == *label)
                    .collect();
                if let Some(node) = unique(matches)
                    && let Some(supported) = feature_status(node)
                {
                    add(
                        source,
                        page,
                        out,
                        node,
                        property,
                        json!(supported),
                        interface(source),
                        &[],
                    );
                    if let Some(types) = unique(select(node, ".geap-supported-type")) {
                        let values: Vec<_> = text(types)
                            .split(',')
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty())
                            .collect();
                        add(
                            source,
                            page,
                            out,
                            types,
                            &format!("{property}_types"),
                            json!(values),
                            interface(source),
                            &[],
                        );
                    }
                }
            }
        }
    }
    for (label, property) in [
        ("APIs", "endpoint_support"),
        ("Consumption options", "deployment_options"),
    ] {
        if let Some(row) = named_row(table, label) {
            let mut values = Map::new();
            let mut duplicate = false;
            for node in select(row, "li.geap-feature") {
                if let Some(status) = feature_status(node) {
                    let name = feature_label(node);
                    if name.is_empty() || values.insert(name, json!(status)).is_some() {
                        duplicate = true;
                    }
                }
            }
            if !values.is_empty() && !duplicate {
                add(
                    source,
                    page,
                    out,
                    row,
                    property,
                    Value::Object(values),
                    interface(source),
                    &[],
                );
            }
        }
    }
    google_input_details(source, page, out, table);
    if let Some(row) = named_row(table, "Versions") {
        let codes: BTreeSet<_> = select(row, "code").into_iter().map(text).collect();
        if codes.len() == 1 && codes.contains("gemini-2.5-pro") {
            for (prefix, property) in [
                ("Launch stage:", "launch_stage"),
                ("Retirement date", "retirement_date"),
            ] {
                let matches = select(row, "li")
                    .into_iter()
                    .filter(|n| text(*n).starts_with(prefix))
                    .collect();
                if let Some(node) = unique(matches)
                    && let Some((_, raw)) = text(node).split_once(':')
                {
                    let value = if property == "retirement_date" {
                        date(raw.trim())
                    } else {
                        Some(raw.trim().to_owned())
                    };
                    if let Some(value) = value {
                        add(
                            source,
                            page,
                            out,
                            node,
                            property,
                            json!(value),
                            interface(source),
                            &["Google Cloud deployment lifecycle"],
                        );
                    }
                }
            }
        }
    }
    if let Some(row) = named_row(table, "Supported regions") {
        let codes: BTreeSet<_> = select(row, "code").into_iter().map(text).collect();
        if !codes.is_empty() {
            add(
                source,
                page,
                out,
                row,
                "regions",
                json!(codes),
                interface(source),
                &[],
            );
        }
    }
}

fn google_input_details(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    table: ElementRef<'_>,
) {
    if let Some(row) = named_row(table, "Input size limit")
        && let Some(node) = cells(row).last().copied()
    {
        let value = text(node);
        let parts: Vec<_> = value.split_whitespace().collect();
        if let [amount, unit] = parts.as_slice()
            && let Some((amount, Precision::Exact)) = count(amount)
        {
            add(
                source,
                page,
                out,
                node,
                "input_size_limit",
                json!({"value":amount,"unit":unit}),
                interface(source),
                &[],
            );
        }
    }
    for row in select(table, "tr") {
        let Some(kind_cell) = unique(select(row, "td.geap-technical-type")) else {
            continue;
        };
        let Some(kind) = unique(select(kind_cell, "b")) else {
            continue;
        };
        let kind = text(kind);
        let Some(node) = cells(row).last().copied() else {
            continue;
        };
        if kind == "Parameter defaults" {
            let mut values = Map::new();
            for item in select(node, "li") {
                if let Some((name, raw)) = text(item).split_once(':')
                    && let Some(value) = parameter_value(raw.trim())
                {
                    values.insert(name.trim().to_owned(), value);
                }
            }
            if !values.is_empty() {
                add(
                    source,
                    page,
                    out,
                    node,
                    "sampling_parameters",
                    Value::Object(values),
                    interface(source),
                    &[],
                );
            }
            continue;
        }
        if !matches!(kind.as_str(), "Image" | "Text" | "Video" | "Audio") {
            continue;
        }
        let mut constraints = Map::new();
        for item in select(node, "li") {
            if let Some((name, raw)) = text(item).split_once(':') {
                let raw = raw.trim();
                let value = if name.trim() == "Supported MIME types" {
                    json!(
                        raw.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                    )
                } else if let Some((value, Precision::Exact)) = count(raw) {
                    json!(value)
                } else {
                    json!(raw)
                };
                constraints.insert(name.trim().to_owned(), value);
            }
        }
        if !constraints.is_empty() {
            add(
                source,
                page,
                out,
                node,
                &format!("input_constraints.{}", kind.to_lowercase()),
                Value::Object(constraints),
                interface(source),
                &[
                    "Upload paths and approximate duration qualifiers are retained in the source values",
                ],
            );
        }
    }
}

fn parameter_value(raw: &str) -> Option<Value> {
    if let Some(value) = raw.strip_suffix(" (fixed)") {
        return value
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map(|v| json!({"fixed":v}));
    }
    let (range, default) = raw.split_once(" (default ")?;
    let default: f64 = default.strip_suffix(')')?.parse().ok()?;
    let (minimum, maximum) = range.split_once(['-', '–'])?;
    let (minimum, maximum) = (minimum.parse::<f64>().ok()?, maximum.parse::<f64>().ok()?);
    (minimum.is_finite()
        && maximum.is_finite()
        && default.is_finite()
        && minimum <= default
        && default <= maximum)
        .then(|| json!({"minimum":minimum,"maximum":maximum,"default":default}))
}

fn amazon_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    let mut candidates = Vec::new();
    for table in page.select("#main-col-body table") {
        let headers = select(table, "tr")
            .into_iter()
            .filter(|row| cells(*row).iter().any(|c| c.value().name() == "th"))
            .collect();
        let Some(header) = unique(headers) else {
            continue;
        };
        let header = cells(header);
        let columns: Vec<_> = header
            .iter()
            .enumerate()
            .filter(|(_, n)| text(**n) == "Amazon Nova Pro")
            .map(|(i, _)| i)
            .collect();
        let [column] = columns.as_slice() else {
            continue;
        };
        let Some(row) = named_row(table, "Model ID") else {
            continue;
        };
        let row = cells(row);
        if row.len() == header.len() && text(row[*column]) == "amazon.nova-pro-v1:0" {
            candidates.push((table, *column, header.len()));
        }
    }
    let [(table, column, width)] = candidates.as_slice() else {
        return;
    };
    for (label, property) in [
        ("Document Support", "document_formats"),
        ("Converse API", "converse_api"),
        ("InvokeAPI", "invoke_api"),
        ("Streaming", "streaming"),
        ("Batch Inference", "batch_inference"),
        ("Fine Tuning", "fine_tuning"),
        ("Provisioned Throughput", "provisioned_throughput"),
        ("Bedrock Knowledge Bases", "bedrock_knowledge_bases"),
        ("Bedrock Agents", "bedrock_agents"),
    ] {
        let Some(row) = named_row(*table, label) else {
            continue;
        };
        let row = cells(row);
        if row.len() != *width {
            continue;
        }
        let node = row[*column];
        let raw = text(node);
        let value = if property == "document_formats" {
            json!(
                raw.split(',')
                    .map(str::trim)
                    .map(str::to_lowercase)
                    .collect::<Vec<_>>()
            )
        } else {
            match raw.as_str() {
                "Yes" => json!(true),
                "No" => json!(false),
                _ => continue,
            }
        };
        add(
            source,
            page,
            out,
            node,
            property,
            value,
            interface(source),
            &[],
        );
    }
}

fn meta_identity(page: &Page) -> bool {
    let matches: Vec<_> = page
        .select("table tbody tr")
        .into_iter()
        .filter(|row| {
            cells(*row)
                .first()
                .is_some_and(|n| text(*n) == "Llama 3.3 (text only)")
        })
        .collect();
    matches.len() == 1 && find(page, "li", "70B Instruct:").is_some()
}

fn anthropic_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    let Some(overview) = find(
        page,
        "main article p.post-text",
        "Claude Opus 4 and Sonnet 4 are hybrid models offering",
    ) else {
        return;
    };
    if let Some((_, modes)) = text(overview).split_once("offering two modes: ")
        && let Some((modes, _)) = modes.split_once('.')
    {
        add(
            source,
            page,
            out,
            overview,
            "reasoning_modes",
            json!(modes.split(" and ").map(str::trim).collect::<Vec<_>>()),
            ClaimScope::Model,
            &["Shared statement explicitly naming Claude Opus 4 and Claude Sonnet 4 at launch"],
        );
    }
    if let Some(node) = find(
        page,
        "main article li",
        "Extended thinking with tool use (beta)",
    ) && text(node).contains("Both models can use tools")
    {
        add(
            source,
            page,
            out,
            node,
            "extended_thinking_tool_use",
            json!(true),
            interface(source),
            &["Beta capability at the Claude 4 launch"],
        );
    }
    if let Some(node) = find(page, "main article li", "New model capabilities")
        && text(node).contains("Both models can use tools in parallel")
    {
        add(
            source,
            page,
            out,
            node,
            "parallel_tool_calls",
            json!(true),
            interface(source),
            &["Shared Claude 4 launch capability; application tool access is required"],
        );
    }
}

fn xai_launch_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    if let Some(node) = find(
        page,
        "div.text-center > p",
        "We're thrilled to introduce grok-code-fast-1",
    ) && text(node).contains("reasoning model")
    {
        add(
            source,
            page,
            out,
            node,
            "reasoning",
            json!(true),
            ClaimScope::Model,
            &["Original Grok Code Fast 1 launch; current aliases may name a different model"],
        );
    }
}

// Meta-specific extraction follows the exact model-card identity guard above.
fn meta_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    if let Some(node) = find(page, "p", "Model Architecture:") {
        let value = text(node);
        if let Some(body) = value.strip_prefix("Model Architecture:")
            && let Some((architecture, tuning)) = body.trim().split_once(". ")
        {
            add(
                source,
                page,
                out,
                node,
                "architecture",
                serde_json::json!(architecture),
                ClaimScope::Model,
                &[],
            );
            if let Some(methods) = tuning
                .strip_prefix("The tuned versions use ")
                .and_then(|value| value.split_once(" to align ").map(|(methods, _)| methods))
            {
                add(
                    source,
                    page,
                    out,
                    node,
                    "training_methods",
                    serde_json::json!(methods.split(" and ").collect::<Vec<_>>()),
                    ClaimScope::Model,
                    &[],
                );
            }
        }
        content::push(
            out,
            page,
            node,
            PassageTopic::Overview,
            Some("Model Information"),
            &source.records,
        );
    }

    if let Some(node) = find(page, "p", "Supported languages:") {
        let value = text(node);
        let languages: Option<Vec<_>> =
            value.strip_prefix("Supported languages:").and_then(|list| {
                list.trim()
                    .trim_end_matches('.')
                    .split(',')
                    .map(|name| {
                        let name = name.trim().strip_prefix("and ").unwrap_or(name.trim());
                        match name {
                            "English" => Some("en"),
                            "German" => Some("de"),
                            "French" => Some("fr"),
                            "Italian" => Some("it"),
                            "Portuguese" => Some("pt"),
                            "Hindi" => Some("hi"),
                            "Spanish" => Some("es"),
                            "Thai" => Some("th"),
                            _ => None,
                        }
                    })
                    .collect()
            });
        if let Some(languages) = languages {
            add(
                source,
                page,
                out,
                node,
                "languages",
                serde_json::json!(languages),
                ClaimScope::Model,
                &[],
            );
        } else {
            out.warnings.push("The supported-language list contains an unmapped name; the complete language claim was skipped.".into());
        }
    }

    if let Some(node) = find(page, "p", "Llama 3.3 model") {
        let value = text(node);
        if let Some(attention) = value
            .split_once("All model versions use ")
            .and_then(|(_, rest)| rest.split_once(" for ").map(|(attention, _)| attention))
        {
            add(
                source,
                page,
                out,
                node,
                "attention_type",
                serde_json::json!(attention),
                ClaimScope::Model,
                &[],
            );
        }
    }

    if let Some(node) = find(page, "p", "Overview: Llama 3.3 was pretrained") {
        let value = text(node);
        if let Some(body) = value.strip_prefix("Overview:")
            && let Some((pretraining, fine_tuning)) = body.trim().split_once(". ")
        {
            add(
                source,
                page,
                out,
                node,
                "training_data",
                serde_json::json!({"pretraining": pretraining, "fine_tuning": fine_tuning}),
                ClaimScope::Model,
                &[
                    "Source wording preserves approximate token counts and qualified example counts.",
                ],
            );
        }
        content::push(
            out,
            page,
            node,
            PassageTopic::Training,
            Some("Training Data"),
            &source.records,
        );
    }

    if let Some(node) = find(page, "p", "Intended Use Cases") {
        let value = text(node);
        if let Some(body) = value.strip_prefix("Intended Use Cases") {
            let uses: Vec<_> = body
                .trim()
                .split(". ")
                .filter_map(|sentence| {
                    if sentence.starts_with("Llama 3.3 is intended for ")
                        || sentence.starts_with("The Llama 3.3 model also supports ")
                    {
                        Some(sentence)
                    } else if sentence
                        .starts_with("Instruction tuned text only models are intended for ")
                    {
                        sentence
                            .split_once(", whereas pretrained models ")
                            .map(|(instruction_use, _)| instruction_use)
                    } else {
                        None
                    }
                })
                .collect();
            if !uses.is_empty() {
                add(
                    source,
                    page,
                    out,
                    node,
                    "intended_uses",
                    serde_json::json!(uses),
                    ClaimScope::Model,
                    &[
                        "Instruction-tuned and model-specific uses; the generic pretrained-model clause is excluded.",
                    ],
                );
            }
        }
        content::push(
            out,
            page,
            node,
            PassageTopic::Usage,
            Some("Intended Use"),
            &source.records,
        );
    }

    for (prefix, property, heading, topic) in [
        (
            "Out-of-scope",
            "usage_constraints",
            "Intended Use",
            Some(PassageTopic::Limitations),
        ),
        (
            "**Note: Llama 3.3 has been trained",
            "language_adaptation_guidance",
            "Intended Use",
            None,
        ),
        (
            "Fine-tuning data",
            "safety_training",
            "Llama 3.3 instruct",
            None,
        ),
        (
            "Refusals and Tone",
            "safety_response_guidance",
            "Llama 3.3 instruct",
            None,
        ),
        (
            "Large language models, including Llama 3.3",
            "deployment_guidance",
            "Llama 3.3 systems",
            Some(PassageTopic::Usage),
        ),
        (
            "But Llama 3.3 is a new technology",
            "limitations",
            "Ethical Considerations and Limitations",
            Some(PassageTopic::Limitations),
        ),
    ] {
        if let Some(node) = find(page, "p", prefix) {
            add(
                source,
                page,
                out,
                node,
                property,
                serde_json::json!(text(node)),
                ClaimScope::Model,
                &["Model-card guidance; no quantitative safety guarantee is implied."],
            );
            if let Some(topic) = topic {
                content::push(out, page, node, topic, Some(heading), &source.records);
            }
        }
    }

    if let Some(node) = find(page, "p", "License A custom commercial license")
        && let Some(link) = unique(select(node, "a[href]"))
        && let Some(href) = link.attr("href")
    {
        add(
            source,
            page,
            out,
            link,
            "license_url",
            serde_json::json!(href),
            ClaimScope::Model,
            &[
                "License destination linked by the model card; no additional license terms are inferred.",
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{DocumentKind, Format, page};

    fn run(id: &str, format: Format, raw: &str) -> Extraction {
        let source = PageSource {
            id: id.into(),
            vendor: "openai".into(),
            kind: DocumentKind::ModelPage,
            url: "https://example.com/model".into(),
            format,
            records: vec!["model:target".into()],
        };
        let page = Page::parse(&source, raw.as_bytes()).unwrap();
        let mut out = page::metadata(&source, &page).unwrap();
        extract(&source, &page, &mut out).unwrap();
        out
    }

    #[test]
    fn complete_original_paragraph_and_feature_scope_are_preserved() {
        let ending = "Source words. ".repeat(25);
        let raw = format!(
            "<main><h1>GPT-4.1</h1><p>GPT-4.1 excels at <em>instruction following</em>.\n {ending}Without truncation.</p><section><div>Features</div><div><div>Streaming</div><div>Supported</div></div></section></main>"
        );
        let out = run("openai-gpt41-model", Format::Html, &raw);
        assert_eq!(
            out.passages[0].text,
            format!("GPT-4.1 excels at instruction following.\n {ending}Without truncation.")
        );
        assert!(out.passages[0].text.len() > 240);
        let streaming = out
            .claims
            .iter()
            .find(|c| c.property == "streaming")
            .unwrap();
        assert!(
            matches!(&streaming.scope, ClaimScope::Interface { id } if id == "https://example.com/model")
        );
    }

    #[test]
    fn google_detail_rows_require_exact_model_and_handle_reused_ids() {
        let raw = r#"<table class="geap-model-table"><tr><th>Model ID</th><td>gemini-2.5-flash</td></tr><tr><th>Capabilities</th><td><li class="geap-feature">Thinking<span class="geap-not-supported">Not supported</span></li></td></tr></table>
            <table class="geap-model-table"><tr><th>Model ID</th><td>gemini-2.5-pro</td></tr><tr><th id="capabilities">Capabilities</th><td><li class="geap-feature"><a>Thinking</a><span class="geap-supported">Supported</span></li></td></tr><tr><th id="capabilities">APIs</th><td><li class="geap-feature">Gemini Live API<span class="geap-not-supported">Not supported</span></li></td></tr></table>"#;
        let out = run("google-gemini25-model", Format::Html, raw);
        assert_eq!(
            out.claims
                .iter()
                .find(|c| c.property == "reasoning")
                .unwrap()
                .value,
            json!(true)
        );
        assert_eq!(
            out.claims
                .iter()
                .find(|c| c.property == "endpoint_support")
                .unwrap()
                .value,
            json!({"Gemini Live API":false})
        );
        assert!(
            out.claims
                .iter()
                .all(|c| matches!(c.scope, ClaimScope::Interface { .. }))
        );
    }

    #[test]
    fn amazon_passage_binding_requires_original_model_identifier() {
        let raw = r#"<section class="blog-post-content"><pre><code>model = "amazon.nova-pro-v1:0"</code></pre><p>Amazon Nova Pro – a model for text and visual intelligence.</p></section>"#;
        let original = run("amazon-nova-launch", Format::Html, raw);
        assert!(!original.passages[0].records.is_empty());
        let updated = run(
            "amazon-nova-launch",
            Format::Html,
            &raw.replace("amazon.nova-pro-v1:0", "amazon.nova-pro-v2:0"),
        );
        assert_eq!(updated.passages[0].text, original.passages[0].text);
        assert!(updated.passages[0].records.is_empty());
    }

    #[test]
    fn meta_languages_come_from_guarded_model_card() {
        let raw = "| Model | Params |\n|---|---|\n| Llama 3.3 (text only) | 70B |\n\n* **70B Instruct: December 6, 2024**\n\n**Supported languages:** English, and Thai.\n";
        let out = run("meta-llama33-card", Format::Markdown, raw);
        assert_eq!(
            out.claims
                .iter()
                .find(|c| c.property == "languages")
                .unwrap()
                .value,
            json!(["en", "th"])
        );
        assert!(
            run(
                "meta-llama33-card",
                Format::Markdown,
                &raw.replace("Llama 3.3", "Llama 3.2")
            )
            .claims
            .is_empty()
        );
    }
}

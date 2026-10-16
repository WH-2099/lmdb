use anyhow::Result;
use scraper::ElementRef;
use serde_json::{Map, Value, json};

use super::asia::{cells, elements, model_heading, number, scaled, section};
use super::content;
use super::model::{
    Claim, ClaimScope, Extraction, Format, PageEvidence, PageSource, PassageTopic, Precision,
};
use super::page::{Page, text};

pub fn extract(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    match source.id.as_str() {
        "deepseek-blog" => blog_passages(
            source,
            page,
            out,
            &[
                (
                    "article p",
                    "🔹 DeepSeek-V3.2: Official successor",
                    PassageTopic::Availability,
                    "DeepSeek-V3.2 Release",
                    true,
                ),
                (
                    "article p",
                    "🔹 DeepSeek-V3.2 is our first model",
                    PassageTopic::Capabilities,
                    "Thinking in Tool-Use",
                    true,
                ),
                (
                    "article p",
                    "🔹 V3.2: Same usage pattern",
                    PassageTopic::Usage,
                    "API Update",
                    true,
                ),
            ],
        ),
        "qwen-blog" => blog_passages(
            source,
            page,
            out,
            &[
                (
                    "article.post-single p",
                    "We are open-weighting two MoE models:",
                    PassageTopic::Overview,
                    "Introduction",
                    false,
                ),
                (
                    "article.post-single p",
                    "The post-trained models, such as",
                    PassageTopic::Availability,
                    "Introduction",
                    false,
                ),
                (
                    "article.post-single p",
                    "In terms of pretraining, the dataset for Qwen3",
                    PassageTopic::Training,
                    "Pre-training",
                    false,
                ),
                (
                    "article.post-single p",
                    "To develop the hybrid model capable",
                    PassageTopic::Training,
                    "Post-training",
                    false,
                ),
            ],
        ),
        "nvidia-blog" => blog_passages(
            source,
            page,
            out,
            &[
                (
                    "article#post-90771 p",
                    "Launched today, NVIDIA Nemotron 3 Super",
                    PassageTopic::Overview,
                    "NVIDIA Nemotron 3 Super",
                    true,
                ),
                (
                    "article#post-90771 p",
                    "Nemotron 3 Super has a ",
                    PassageTopic::Capabilities,
                    "NVIDIA Nemotron 3 Super",
                    true,
                ),
                (
                    "article#post-90771 p",
                    "NVIDIA is releasing Nemotron 3 Super",
                    PassageTopic::License,
                    "Open Weights, Data and Recipes",
                    true,
                ),
                (
                    "article#post-90771 p",
                    "NVIDIA Nemotron 3 Super, part of",
                    PassageTopic::Availability,
                    "Availability",
                    true,
                ),
            ],
        ),
        "deepseek-card" => deepseek_details(source, page, out),
        "qwen-card" => qwen_details(source, page, out),
        "nvidia-card" | "nvidia-page" => nvidia_details(source, page, out),
        "west-mistral-blog"
        | "west-mistral-card"
        | "west-cohere-blog"
        | "west-cohere-doc"
        | "west-microsoft-blog"
        | "west-microsoft-card" => west_details(source, page, out)?,
        _ => {}
    }
    Ok(())
}

type Selection<'a> = (&'a str, &'a str, PassageTopic, &'a str, bool);

fn blog_passages(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    selections: &[Selection<'_>],
) {
    for (selector, prefix, topic, heading, exact_model) in selections {
        let matches: Vec<_> = page
            .select(selector)
            .into_iter()
            .filter(|n| text(*n).starts_with(prefix))
            .collect();
        if matches.len() == 1 {
            content::push(
                out,
                page,
                matches[0],
                *topic,
                Some(heading),
                if *exact_model { &source.records } else { &[] },
            );
        } else {
            out.warnings.push(format!(
                "Selected passage under {heading:?} had {} matches; skipped.",
                matches.len()
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn detail(
    source: &PageSource,
    out: &mut Extraction,
    property: &str,
    value: Value,
    unit: Option<&str>,
    precision: Precision,
    scope: ClaimScope,
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
            scope: scope.clone(),
            conditions: conditions.iter().map(|s| (*s).into()).collect(),
            evidence: evidence.clone(),
        });
    }
}

fn intrinsic(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    node: ElementRef<'_>,
    property: &str,
    value: Value,
) {
    detail(
        source,
        out,
        property,
        value,
        None,
        Precision::Exact,
        ClaimScope::Model,
        &[],
        page.evidence(node),
    );
}

fn sampling(text: &str, names: &[(&str, &str)]) -> Value {
    let mut values = Map::new();
    for (label, property) in names {
        let Some((_, after)) = text.split_once(label) else {
            continue;
        };
        let Some(after) = after.trim_start().strip_prefix('=') else {
            continue;
        };
        let Some(token) = after.split_whitespace().next() else {
            continue;
        };
        let token = token.trim_end_matches([',', ';']);
        if let Ok(value) = token.parse::<serde_json::Number>() {
            values.insert((*property).into(), Value::Number(value));
        }
    }
    Value::Object(values)
}

fn deepseek_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    if !model_heading(page, "DeepSeek-V3.2: Efficient Reasoning & Agentic AI", out) {
        return;
    }
    for node in elements(&section(page, "Introduction"), "li") {
        let value = text(node);
        if let Some((attention, _)) = value.split_once(":")
            && attention.starts_with("DeepSeek Sparse Attention")
        {
            intrinsic(
                source,
                page,
                out,
                node,
                "attention_mechanism",
                json!(attention),
            );
        }
    }
    for node in elements(&section(page, "Chat Template"), "li") {
        let value = text(node);
        if value.starts_with("This release does not include a Jinja-format chat template.") {
            intrinsic(
                source,
                page,
                out,
                node,
                "limitations.chat_template",
                json!(value),
            );
        } else if value.starts_with("The output parsing function included in the code") {
            intrinsic(
                source,
                page,
                out,
                node,
                "limitations.output_parser",
                json!(value),
            );
        }
    }
    for node in elements(&section(page, "How to Run Locally"), "li") {
        let value = text(node);
        if value.starts_with("For local deployment, we recommend setting the sampling parameters") {
            let values = sampling(
                &value,
                &[("temperature", "temperature"), ("top_p", "top_p")],
            );
            if values.as_object().is_some_and(|v| !v.is_empty()) {
                detail(
                    source,
                    out,
                    "recommended_sampling",
                    values,
                    None,
                    Precision::Exact,
                    ClaimScope::Model,
                    &["Recommended local deployment settings; not defaults or limits."],
                    page.evidence(node),
                );
            }
        }
    }
}

fn qwen_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    if !model_heading(page, "Qwen3-8B", out) {
        return;
    }
    for node in elements(&section(page, "Model Overview"), "li") {
        let value = text(node);
        let Some((label, value)) = value.split_once(": ") else {
            continue;
        };
        match label {
            "Type" => intrinsic(source, page, out, node, "architecture_type", json!(value)),
            "Training Stage" => intrinsic(
                source,
                page,
                out,
                node,
                "training_stages",
                json!(value.split(" & ").collect::<Vec<_>>()),
            ),
            "Number of Layers" => {
                if let Some(value) = number(value) {
                    intrinsic(source, page, out, node, "layer_count", json!(value));
                }
            }
            "Number of Paramaters (Non-Embedding)" => {
                if let Some(value) = scaled(value) {
                    detail(
                        source,
                        out,
                        "non_embedding_parameter_count",
                        json!(value),
                        Some("parameters"),
                        Precision::Approximate,
                        ClaimScope::Model,
                        &["Advertised rounded non-embedding count."],
                        page.evidence(node),
                    );
                }
            }
            "Number of Attention Heads (GQA)" => {
                if let Some((query, key_value)) = value.split_once(" for Q and ")
                    && let Some(key_value) = key_value.strip_suffix(" for KV")
                    && let (Some(query), Some(key_value)) = (number(query), number(key_value))
                {
                    intrinsic(
                        source,
                        page,
                        out,
                        node,
                        "attention_heads",
                        json!({"query":query,"key_value":key_value}),
                    );
                }
            }
            _ => {}
        }
    }
    for node in elements(&section(page, "Processing Long Texts"), "p") {
        let value = text(node);
        let value = value.strip_prefix("[!NOTE] ").unwrap_or(&value);
        if let Some(rest) =
            value.strip_prefix("The default max_position_embeddings in config.json is set to ")
            && let Some(token) = rest.split('.').next()
            && let Some(count) = number(token)
        {
            detail(
                source,
                out,
                "configured_position_embeddings",
                json!(count),
                Some("tokens"),
                Precision::Exact,
                ClaimScope::Model,
                &[
                    "Default artifact configuration; distinct from native context and output limits.",
                ],
                page.evidence(node),
            );
        }
        if value.contains("All the notable open-source frameworks implement static YaRN") {
            detail(
                source,
                out,
                "limitations.context_extension",
                json!(value),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &["Static YaRN context extension; shorter inputs may be affected."],
                page.evidence(node),
            );
        }
    }
    for node in elements(&section(page, "Best Practices"), "li") {
        let value = text(node);
        let mode = if value.starts_with("For thinking mode") {
            Some("Thinking mode enabled.")
        } else if value.starts_with("For non-thinking mode") {
            Some("Thinking mode disabled.")
        } else {
            None
        };
        if let Some(mode) = mode {
            let values = sampling(
                &value,
                &[
                    ("Temperature", "temperature"),
                    ("TopP", "top_p"),
                    ("TopK", "top_k"),
                    ("MinP", "min_p"),
                ],
            );
            if values.as_object().is_some_and(|v| !v.is_empty()) {
                detail(
                    source,
                    out,
                    "recommended_sampling",
                    values,
                    None,
                    Precision::Exact,
                    ClaimScope::Model,
                    &[mode, "Publisher recommendation; not an endpoint default."],
                    page.evidence(node),
                );
            }
        }
        if value
            .strip_prefix("Adequate Output Length")
            .is_some_and(|suffix| suffix.trim_start().starts_with(':'))
        {
            for (prefix, condition) in [
                (
                    "We recommend using an output length of ",
                    "Recommended for most queries.",
                ),
                (
                    "we suggest setting the max output length to ",
                    "Recommended for complex mathematics/programming benchmarks.",
                ),
            ] {
                if let Some((_, rest)) = value.split_once(prefix)
                    && let Some(count) = number(rest)
                {
                    detail(
                        source,
                        out,
                        "recommended_output_tokens",
                        json!(count),
                        Some("tokens"),
                        Precision::Exact,
                        ClaimScope::Model,
                        &[
                            condition,
                            "Recommendation, not a maximum output capability.",
                        ],
                        page.evidence(node),
                    );
                }
            }
        }
    }
}

fn nvidia_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    let expected = if source.id == "nvidia-card" {
        "NVIDIA-Nemotron-3-Super-120B-A12B-BF16"
    } else {
        "NVIDIA-Nemotron-3-Super-120B-A12B"
    };
    if !model_heading(page, expected, out) {
        return;
    }
    for row in elements(&section(page, "Model Summary"), "tr") {
        let columns = cells(row);
        let [label, value] = columns.as_slice() else {
            continue;
        };
        let value = text(*value);
        match text(*label).as_str() {
            "Architecture" => {
                intrinsic(
                    source,
                    page,
                    out,
                    row,
                    "architecture_description",
                    json!(value),
                );
                if let Some((routing, rest)) = value.split_once(" - ")
                    && let Some((layers, prediction)) = rest.split_once(" hybrid with ")
                {
                    let mut components = vec![routing];
                    components.extend(layers.split(" + "));
                    components.push(prediction);
                    intrinsic(
                        source,
                        page,
                        out,
                        row,
                        "architecture_components",
                        json!(components),
                    );
                }
            }
            "Minimum GPU Requirement" => detail(
                source,
                out,
                "deployment.minimum_gpu",
                json!(value),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &[
                    "Minimum GPU guidance for the documented BF16 deployment; quantized variants have separate requirements.",
                ],
                page.evidence(row),
            ),
            "Supported Languages" => intrinsic(
                source,
                page,
                out,
                row,
                "language_names",
                json!(value.split(',').map(str::trim).collect::<Vec<_>>()),
            ),
            "Reasoning Mode" => intrinsic(
                source,
                page,
                out,
                row,
                "reasoning_configuration",
                json!(value),
            ),
            "Speculative Decoding" => {
                intrinsic(source, page, out, row, "speculative_decoding", json!(value))
            }
            _ => {}
        }
    }
    for node in elements(&section(page, "Model Overview"), "li") {
        let value = text(node);
        for (prefix, property) in [
            (
                "The post-training data has a cutoff date of ",
                "training_data_cutoff.post_training",
            ),
            (
                "The pre-training data has a cutoff date of ",
                "training_data_cutoff.pre_training",
            ),
        ] {
            if let Some(date) = value.strip_prefix(prefix) {
                intrinsic(
                    source,
                    page,
                    out,
                    node,
                    property,
                    json!(date.trim_end_matches('.')),
                );
            }
        }
    }
    for node in elements(&section(page, "Model Design"), "p") {
        let value = text(node);
        if let Some((_, rest)) = value.split_once("The Super model is pre-trained using ")
            && let Some((precision, _)) = rest.split_once(" quantization")
        {
            detail(
                source,
                out,
                "training_precision",
                json!(precision),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &["Pre-training arithmetic; distinct from the downloaded artifact precision."],
                page.evidence(node),
            );
        }
    }
    for node in section(page, "Training Methodology") {
        let value = text(node);
        if node.value().name() == "p"
            && let Some(rest) = value.strip_prefix("Stage ")
            && let Some((order, stage)) = rest.split_once(": ")
            && let Some(order) = number(order)
        {
            intrinsic(
                source,
                page,
                out,
                node,
                &format!("training_stage.{order}"),
                json!(stage),
            );
        }
        for item in elements(&[node], "li") {
            let value = text(item);
            if value.starts_with("The model underwent multi-environment reinforcement learning") {
                detail(
                    source,
                    out,
                    "training_method.reinforcement_learning",
                    json!(value),
                    None,
                    Precision::Exact,
                    ClaimScope::Model,
                    &["Reinforcement learning stage."],
                    page.evidence(item),
                );
            } else if value.starts_with("The model was further fine-tuned on synthetic") {
                detail(
                    source,
                    out,
                    "training_data.supervised_fine_tuning",
                    json!(value),
                    None,
                    Precision::Exact,
                    ClaimScope::Model,
                    &["Supervised fine-tuning stage."],
                    page.evidence(item),
                );
            }
        }
    }
    for node in elements(&section(page, "License/Terms of Use"), "p") {
        let value = text(node);
        if value.starts_with("Governing Download Terms with NIM:")
            || value.starts_with("GOVERNING TERMS:")
        {
            detail(
                source,
                out,
                "license.interface_terms",
                json!(value),
                None,
                Precision::Exact,
                ClaimScope::Interface {
                    id: source.url.clone(),
                },
                &["NIM container or hosted trial terms; separate from the model license."],
                page.evidence(node),
            );
        }
    }
    for node in elements(&section(page, "Software Integration"), "li") {
        let value = text(node);
        for (label, property) in [
            ("Runtime Engine(s): ", "deployment.runtime_engine"),
            ("Operating System(s): ", "deployment.operating_system"),
        ] {
            if let Some(value) = value.strip_prefix(label) {
                detail(
                    source,
                    out,
                    property,
                    json!(value),
                    None,
                    Precision::Exact,
                    ClaimScope::Model,
                    &["Documented integration environment, not an exhaustive compatibility claim."],
                    page.evidence(node),
                );
            }
        }
    }
    for node in elements(&section(page, "Evaluation Dataset"), "li") {
        let value = text(node);
        for (label, property) in [
            (
                "Data Collection Method by dataset: ",
                "evaluation.data_collection",
            ),
            ("Labeling Method by dataset: ", "evaluation.labeling"),
        ] {
            if let Some(value) = value.strip_prefix(label) {
                intrinsic(source, page, out, node, property, json!(value));
            }
        }
    }
}
fn west_details(source: &PageSource, page: &Page, out: &mut Extraction) -> anyhow::Result<()> {
    match source.id.as_str() {
        "west-mistral-blog" => {
            if !page.select("a[href]").iter().any(|n| {
                n.value().attr("href")
                    == Some("https://huggingface.co/mistralai/Magistral-Small-2506")
            }) {
                return Ok(());
            }
            for node in page.select("article p") {
                let value = super::page::text(node);
                let choice = if value.starts_with("We’re releasing the model in two variants:") {
                    Some((PassageTopic::Overview, "Highlights.", false))
                } else if value.starts_with("Magistral is fine-tuned for multi-step logic") {
                    Some((
                        PassageTopic::Capabilities,
                        "Purpose-built for transparent reasoning.",
                        false,
                    ))
                } else if value.starts_with("Magistral Small is an open-weight model") {
                    Some((PassageTopic::Availability, "Availability", true))
                } else {
                    None
                };
                if let Some((topic, heading, exact_model)) = choice {
                    let records = if exact_model {
                        source.records.as_slice()
                    } else {
                        &[]
                    };
                    super::content::push(out, page, node, topic, Some(heading), records);
                }
            }
        }
        "west-mistral-card" => west_mistral_details(source, page, out)?,
        "west-cohere-blog" => {
            let bodies = super::asia::west_flight_bodies(page);
            if bodies.len() != 1 {
                return Ok(());
            }
            let (record, body) = &bodies[0];
            let embedded = Page {
                html: scraper::Html::parse_document(body),
                raw: body.clone(),
                format: Format::Html,
            };
            let before = out.passages.len();
            for node in embedded.select("p") {
                let value = super::page::text(node);
                let choice = if value.starts_with("Today, we’re introducing Command A") {
                    Some((PassageTopic::Overview, "Opening paragraph"))
                } else if value.starts_with("We focused on building Command A") {
                    Some((PassageTopic::Usage, "Scalable efficiency"))
                } else if value.starts_with("We designed Command A") {
                    Some((PassageTopic::Capabilities, "Enterprise-ready capabilities"))
                } else if value.starts_with("Command A is available today") {
                    Some((PassageTopic::Availability, "Availability"))
                } else {
                    None
                };
                if let Some((topic, heading)) = choice {
                    super::content::push(
                        out,
                        &embedded,
                        node,
                        topic,
                        Some(heading),
                        &source.records,
                    );
                }
            }
            let prefix = format!("embedded-rsc:script:self.__next_f.push:record{record}:");
            for passage in &mut out.passages[before..] {
                passage.evidence.locator = format!("{prefix}{}", passage.evidence.locator);
            }
        }
        "west-cohere-doc" => west_cohere_details(source, page, out),
        "west-microsoft-blog" | "west-microsoft-card" => west_microsoft_details(source, page, out)?,
        _ => {}
    }
    Ok(())
}

fn west_mistral_details(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
) -> anyhow::Result<()> {
    if !super::asia::model_heading(page, "Magistral Small 1.0", out) {
        return Ok(());
    }
    west_card_lists(source, page, out)?;
    for node in page.select("p") {
        let value = super::page::text(node);
        if value.starts_with("Building upon Mistral Small") {
            if let Some(stages) = value
                .split_once("undergoing ")
                .and_then(|(_, tail)| tail.split_once(", it's"))
            {
                let values: Vec<_> = stages.0.split(" and ").map(str::trim).collect();
                detail(
                    source,
                    out,
                    "training_stages",
                    serde_json::json!(values),
                    None,
                    Precision::Exact,
                    ClaimScope::Model,
                    &[],
                    page.evidence(node),
                );
            }
        } else if let Some(tail) =
            value.strip_prefix("Magistral Small can be deployed locally, fitting within a ")
            && let Some(devices) = tail.strip_suffix(" once quantized.")
        {
            let devices: Vec<_> = devices.split(" or a ").collect();
            detail(
                source,
                out,
                "deployment_guidance",
                serde_json::json!({"device_alternatives": devices}),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &["once quantized; quantization method and precision are not specified"],
                page.evidence(node),
            );
        }
    }
    for node in super::asia::elements(&super::asia::section(page, "Key Features"), "li") {
        let value = super::page::text(node);
        if let Some(description) = value.strip_prefix("Reasoning:") {
            detail(
                source,
                out,
                "reasoning_behavior",
                serde_json::json!(description.trim()),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &[],
                page.evidence(node),
            );
        } else if let Some(languages) = value
            .strip_prefix("Multilingual:")
            .and_then(|s| s.split_once("including "))
        {
            let languages = west_language_names(languages.1.trim_end_matches('.'));
            detail(
                source,
                out,
                "language_names",
                serde_json::json!(languages),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &[
                    "Prose list retained independently from YAML language codes; the two reported lists differ.",
                ],
                page.evidence(node),
            );
        } else if value.starts_with("Context Window:")
            && let Some(recommendation) = value
                .split_once("recommend setting the maximum model length to ")
                .and_then(|(_, tail)| super::asia::scaled(tail))
        {
            detail(
                source,
                out,
                "recommended_context_window",
                serde_json::json!(recommendation),
                Some("tokens"),
                Precision::Approximate,
                ClaimScope::Model,
                &[
                    "Quality recommendation; source warns performance might degrade beyond this length. This is not the advertised capacity.",
                ],
                page.evidence(node),
            );
        }
    }
    let mut sampling = serde_json::Map::new();
    let mut sampling_evidence = None;
    for node in super::asia::section(page, "Sampling parameters") {
        if node.value().name() != "ul" {
            continue;
        }
        for item in super::asia::elements(&[node], "li") {
            let text = super::page::text(item);
            let Some((name, value)) = text.split_once(':') else {
                continue;
            };
            if !matches!(name.trim(), "top_p" | "temperature" | "max_tokens") {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(value.trim())
                && value.is_number()
            {
                sampling.insert(name.trim().to_owned(), value);
            }
        }
        sampling_evidence = Some(page.evidence(node));
    }
    if !sampling.is_empty()
        && let Some(evidence) = sampling_evidence
    {
        detail(
            source,
            out,
            "recommended_sampling",
            serde_json::Value::Object(sampling),
            None,
            Precision::Exact,
            ClaimScope::Model,
            &[
                "Recommended generation settings; max_tokens is not an asserted service output limit.",
            ],
            evidence,
        );
    }
    Ok(())
}

fn west_card_lists(source: &PageSource, page: &Page, out: &mut Extraction) -> anyhow::Result<()> {
    let raw = page.raw.strip_prefix('\u{feff}').unwrap_or(&page.raw);
    let Some(rest) = raw.strip_prefix("---\n") else {
        return Ok(());
    };
    let Some((yaml, _)) = rest.split_once("\n---") else {
        return Ok(());
    };
    let options = serde_saphyr::options! {
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        merge_keys: serde_saphyr::MergeKeyPolicy::Error,
        reject_unsupported_tags: true,
        budget: serde_saphyr::budget! { max_documents:1, max_aliases:0, max_anchors:0, max_depth:16, max_nodes:10_000, max_total_scalar_bytes:128 * 1024 },
    };
    let values: serde_json::Value = serde_saphyr::from_str_with_options(yaml, options)?;
    for (key, property) in [("language", "languages"), ("base_model", "base_models")] {
        let Some(value) = values.get(key).filter(|value| {
            value
                .as_array()
                .is_some_and(|items| items.iter().all(serde_json::Value::is_string))
        }) else {
            continue;
        };
        let marker = format!("{key}:\n");
        let Some(offset) = raw.find(&marker) else {
            continue;
        };
        let mut end = offset + marker.len();
        for line in raw[end..].split_inclusive('\n') {
            if !line.starts_with("- ") {
                break;
            }
            end += line.len();
        }
        detail(
            source,
            out,
            property,
            value.clone(),
            None,
            Precision::Exact,
            ClaimScope::Model,
            &[],
            PageEvidence {
                locator: format!("markdown-bytes:{offset}..{end}:frontmatter/{key}"),
                excerpt: raw[offset..end].trim_end().to_owned(),
            },
        );
    }
    Ok(())
}

fn west_language_names(value: &str) -> Vec<String> {
    value
        .split(", ")
        .map(|name| {
            name.trim()
                .strip_prefix("and ")
                .unwrap_or(name.trim())
                .to_owned()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

fn west_cohere_details(source: &PageSource, page: &Page, out: &mut Extraction) {
    if !page
        .select("div")
        .iter()
        .any(|node| super::page::text(*node) == "command-a-03-2025")
    {
        return;
    }
    for node in super::asia::section(page, "Description") {
        let value = super::page::text(node);
        if let Some((_, tail)) = value.split_once("only requires ")
            && let Some((count, tail)) = tail.split_once(" GPUs (")
            && let Some((models, _)) = tail.split_once(") to run")
        {
            let models: Vec<_> = models.split(" / ").collect();
            detail(
                source,
                out,
                "deployment_requirements",
                serde_json::json!({"gpu_count_reported":count,"gpu_alternatives":models}),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &["Precision, batching, per-GPU memory and throughput workload are not specified."],
                page.evidence(node),
            );
        }
    }
    for node in super::asia::elements(
        &super::asia::section(page, "What Can Command A Be Used For?"),
        "li",
    ) {
        let value = super::page::text(node);
        if !value.starts_with("Multilingual use cases -") {
            continue;
        }
        if let Some((_, names)) = value.split_once("languages: ")
            && let Some((names, _)) = names.split_once(". It has been trained")
        {
            detail(
                source,
                out,
                "language_names",
                serde_json::json!(west_language_names(names)),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &[],
                page.evidence(node),
            );
        }
    }
    for node in page.select(".model-showcase-grid strong") {
        if super::page::text(node) != "Knowledge Cutoff:" {
            continue;
        }
        if let Some(row) = node.parent().and_then(scraper::ElementRef::wrap) {
            let value = super::page::text(row);
            if let Some(date) = value.strip_prefix("Knowledge Cutoff:")
                && let Some(date) = super::asia::west_date(date.trim())
            {
                detail(
                    source,
                    out,
                    "knowledge_cutoff",
                    serde_json::json!(date),
                    None,
                    Precision::Exact,
                    ClaimScope::Model,
                    &[],
                    page.evidence(row),
                );
            }
        }
    }
}
fn west_microsoft_details(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
) -> anyhow::Result<()> {
    use super::asia::{cells, elements, model_heading, number, scaled, section};
    use super::page::{original_text, text};

    match source.id.as_str() {
        "west-microsoft-blog" => {
            const POINTER: &str =
                "/props/pageProps/apolloState/BlogTopicMessage:message:4357090/body";
            let Some(script) = page.select("script#__NEXT_DATA__").first().copied() else {
                return Ok(());
            };
            let data: Value = serde_json::from_str(&script.text().collect::<String>())?;
            let Some(body) = data.pointer(POINTER).and_then(Value::as_str) else {
                return Ok(());
            };
            let embedded = Page {
                html: scraper::Html::parse_document(body),
                raw: body.to_owned(),
                format: Format::Html,
            };
            let passage_start = out.passages.len();
            let claim_start = out.claims.len();
            for node in embedded.select("body > p") {
                let value = text(node);
                let selected =
                    if value.starts_with("Today we are introducing") && value.contains("Phi-4") {
                        Some((PassageTopic::Overview, "Introducing Phi-4", true))
                    } else if value.starts_with("Phi-4 outperforms comparable") {
                        Some((PassageTopic::Capabilities, "Phi-4 Benchmarks", true))
                    } else if value.starts_with("Azure AI Foundry provides") {
                        Some((
                            PassageTopic::Evaluation,
                            "Enabling AI innovation safely and responsibly",
                            false,
                        ))
                    } else if value.starts_with("Additionally, Phi users can use") {
                        Some((
                            PassageTopic::Usage,
                            "Enabling AI innovation safely and responsibly",
                            false,
                        ))
                    } else {
                        None
                    };
                if let Some((topic, heading, model_specific)) = selected {
                    super::content::push(
                        out,
                        &embedded,
                        node,
                        topic,
                        Some(heading),
                        if model_specific { &source.records } else { &[] },
                    );
                }
            }
            let prefix = format!("embedded-json:script#__NEXT_DATA__:{POINTER}:");
            for passage in &mut out.passages[passage_start..] {
                passage.evidence.locator = format!("{prefix}{}", passage.evidence.locator);
            }
            for claim in &mut out.claims[claim_start..] {
                claim.evidence.locator = format!("{prefix}{}", claim.evidence.locator);
            }
        }
        "west-microsoft-card" => {
            if !model_heading(page, "Phi-4 Model Card", out) {
                return Ok(());
            }
            for row in elements(&section(page, "Model Summary"), "tr") {
                let columns = cells(row);
                if columns.len() != 2 {
                    continue;
                }
                let label = text(columns[0]);
                let value = text(columns[1]);
                let mut precision = Precision::Exact;
                let mut unit = None;
                let mut conditions: Vec<&str> = Vec::new();
                let fact = match label.as_str() {
                    "Architecture" => value
                        .split_once(',')
                        .map(|(_, clause)| ("architecture", json!(clause.trim()))),
                    "Training data" => {
                        precision = Precision::Approximate;
                        unit = Some("tokens");
                        value
                            .split_whitespace()
                            .next()
                            .and_then(|token| token.strip_suffix('T'))
                            .and_then(|base| scaled(&format!("{base}B")))
                            .and_then(|count| count.checked_mul(1000))
                            .map(|count| ("training_tokens", json!(count)))
                    }
                    "GPUs" => {
                        conditions.push(
                            "Training hardware; no deployment hardware requirement is stated.",
                        );
                        number(&value).and_then(|count| {
                            value.split_once(char::is_whitespace).map(|(_, model)| {
                                (
                                    "training_hardware",
                                    json!({"gpu_count": count, "gpu_model": model.trim()}),
                                )
                            })
                        })
                    }
                    "Training time" if value.ends_with(" days") => {
                        unit = Some("days");
                        number(&value).map(|days| ("training_duration", json!(days)))
                    }
                    "Dates" => value.split_once('–').map(|(start, end)| {
                        conditions.push("The source reports month precision.");
                        (
                            "training_period",
                            json!({"start": start.trim(), "end": end.trim()}),
                        )
                    }),
                    "Status" => value
                        .split_once("cutoff dates of ")
                        .and_then(|(_, cutoff)| {
                            let date =
                                cutoff.strip_suffix(" and earlier for publicly available data")?;
                            let (month, year) = date.split_once(' ')?;
                            let month: time::Month = month.parse().ok()?;
                            let year: u16 = year.parse().ok()?;
                            conditions.push(
                                "The stated cutoff applies to publicly available training data.",
                            );
                            conditions.push(
                                "The source reports month precision and includes earlier data.",
                            );
                            Some((
                                "knowledge_cutoff",
                                json!(format!("{year:04}-{:02}", u8::from(month))),
                            ))
                        }),
                    "Description" => {
                        let stages: Vec<_> =
                            ["supervised fine-tuning", "direct preference optimization"]
                                .into_iter()
                                .filter(|stage| value.contains(stage))
                                .collect();
                        (!stages.is_empty()).then(|| ("training_alignment_stages", json!(stages)))
                    }
                    _ => None,
                };
                if let Some((property, value)) = fact {
                    detail(
                        source,
                        out,
                        property,
                        value,
                        unit,
                        precision,
                        ClaimScope::Model,
                        &conditions,
                        page.evidence(row),
                    );
                }
            }

            if let Some(frontmatter) = page
                .raw
                .strip_prefix("---\n")
                .and_then(|raw| raw.split_once("\n---").map(|(yaml, _)| yaml))
                && let Some((_, language_lines)) = frontmatter.split_once("\nlanguage:\n")
            {
                let languages: Vec<_> = language_lines
                    .lines()
                    .take_while(|line| line.starts_with("- "))
                    .map(|line| line.trim_start_matches("- ").trim())
                    .filter(|language| !language.is_empty())
                    .collect();
                if !languages.is_empty() {
                    let raw = format!(
                        "language:\n{}",
                        languages
                            .iter()
                            .map(|language| format!("- {language}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    if let Some(start) = page.raw.find(&raw) {
                        detail(
                            source,
                            out,
                            "languages",
                            json!(languages),
                            None,
                            Precision::Exact,
                            ClaimScope::Model,
                            &["Model card language tags; intended use limitations also apply."],
                            super::model::PageEvidence {
                                locator: format!(
                                    "markdown-bytes:{start}..{}:frontmatter/language",
                                    start + raw.len()
                                ),
                                excerpt: raw,
                            },
                        );
                    }
                }
            }

            let dataset_nodes = section(page, "Training Datasets");
            let data_sources: Vec<_> = elements(&dataset_nodes, "ol > li");
            if let Some(list) = elements(&dataset_nodes, "ol").first().copied() {
                let values: Vec<_> = data_sources
                    .iter()
                    .map(|node| original_text(*node))
                    .collect();
                if !values.is_empty() {
                    detail(
                        source,
                        out,
                        "training_data_sources",
                        json!(values),
                        None,
                        Precision::Exact,
                        ClaimScope::Model,
                        &[],
                        page.evidence(list),
                    );
                }
            }
            for node in elements(&dataset_nodes, "p") {
                let value = text(node);
                if let Some(remainder) = value.strip_prefix("Multilingual data constitutes about ")
                    && let Some((percent, _)) = remainder.split_once('%')
                    && let Ok(percent) = percent.trim().parse::<f64>()
                    && percent.is_finite()
                    && (0.0..=100.0).contains(&percent)
                {
                    detail(
                        source,
                        out,
                        "multilingual_training_data_share",
                        json!(percent),
                        Some("percent"),
                        Precision::Approximate,
                        ClaimScope::Model,
                        &[
                            "Share of overall training data; this does not establish intended multilingual support.",
                        ],
                        page.evidence(node),
                    );
                }
            }
            for node in elements(&section(page, "Approach"), "p") {
                let value = text(node);
                if let Some((_, methods)) = value.split_once("combination of ")
                    && let Some((methods, _)) = methods.split_once(", including")
                {
                    let methods: Vec<_> = methods.split(" and ").map(str::trim).collect();
                    detail(
                        source,
                        out,
                        "safety_post_training_methods",
                        json!(methods),
                        None,
                        Precision::Exact,
                        ClaimScope::Model,
                        &[],
                        page.evidence(node),
                    );
                    super::content::push(
                        out,
                        page,
                        node,
                        PassageTopic::Training,
                        Some("Safety / Approach"),
                        &source.records,
                    );
                }
            }
            for row in elements(&section(page, "Intended Use"), "tr") {
                let columns = cells(row);
                if columns.len() != 2 || text(columns[0]) != "Primary Use Cases" {
                    continue;
                }
                let value = text(columns[1]);
                if let Some((_, list)) = value.split_once("require:") {
                    let requirements: Vec<_> = list
                        .split('.')
                        .map(str::trim)
                        .filter(|item| {
                            !item.is_empty() && !item.bytes().all(|b| b.is_ascii_digit())
                        })
                        .collect();
                    if !requirements.is_empty() {
                        detail(
                            source,
                            out,
                            "intended_use",
                            json!({"requirements": requirements}),
                            None,
                            Precision::Exact,
                            ClaimScope::Model,
                            &[
                                "Primarily English; a building block for generative AI features and language model research.",
                            ],
                            page.evidence(row),
                        );
                    }
                }
            }
            for node in elements(&section(page, "Responsible AI Considerations"), "li") {
                let value = text(node);
                if value.starts_with("Quality of Service:") {
                    if let Some((_, language)) = value.split_once("trained primarily on ")
                        && let Some((language, _)) = language.split_once(" text")
                    {
                        detail(
                            source,
                            out,
                            "primary_training_language",
                            json!(language),
                            None,
                            Precision::Exact,
                            ClaimScope::Model,
                            &[
                                "Non-English languages perform worse; less represented English varieties may perform worse than standard American English.",
                            ],
                            page.evidence(node),
                        );
                    }
                    if value.contains("not intended to support multilingual use") {
                        detail(
                            source,
                            out,
                            "intended_multilingual_support",
                            json!(false),
                            None,
                            Precision::Exact,
                            ClaimScope::Model,
                            &[],
                            page.evidence(node),
                        );
                    }
                    super::content::push(
                        out,
                        page,
                        node,
                        PassageTopic::Limitations,
                        Some("Responsible AI Considerations / Quality of Service"),
                        &source.records,
                    );
                }
            }
            for node in elements(&section(page, "Model Quality"), "p") {
                if text(node).contains("These scores are lower than those reported by Meta") {
                    super::content::push(
                        out,
                        page,
                        node,
                        PassageTopic::Evaluation,
                        Some("Model Quality"),
                        &[],
                    );
                }
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod west_detail_tests {
    use super::*;

    fn fixture(id: &str, raw: &str) -> (PageSource, Page, Extraction) {
        let source = PageSource {
            id: id.into(),
            vendor: "cohere".into(),
            kind: super::super::model::DocumentKind::ModelPage,
            url: "https://docs.cohere.com/docs/command-a".into(),
            format: Format::Html,
            records: vec!["modelsdev:model:cohere/command-a-03-2025".into()],
        };
        let page = Page::parse(&source, raw.as_bytes()).unwrap();
        let out = super::super::page::metadata(&source, &page).unwrap();
        (source, page, out)
    }

    #[test]
    fn embedded_cohere_passage_keeps_original_spacing_and_source_record() {
        let original = "We designed Command A with  spacing.\nIts published wording stays.";
        let body = format!(
            "<h2>Enterprise-ready capabilities</h2><p>{original}</p><a href='https://cohere.com/?model=command-a-03-2025'>Model</a>"
        );
        let stream = format!("7:T{:x},{body}", body.len());
        let raw = format!(
            "<script>self.__next_f.push({})</script>",
            serde_json::json!([1, stream])
        );
        let (source, page, mut out) = fixture("west-cohere-blog", &raw);
        west_details(&source, &page, &mut out).unwrap();
        assert_eq!(out.passages.len(), 1);
        assert_eq!(out.passages[0].text, original);
        assert_eq!(out.passages[0].records, source.records);
        assert!(
            out.passages[0]
                .evidence
                .locator
                .starts_with("embedded-rsc:script:self.__next_f.push:record7:css:")
        );
    }

    #[test]
    fn cohere_human_cutoff_date_is_normalized_without_losing_evidence() {
        let raw = "<div>command-a-03-2025</div><div class='model-showcase-grid'><p><strong>Knowledge Cutoff:</strong> June 1, 2024</p></div>";
        let (source, page, mut out) = fixture("west-cohere-doc", raw);
        west_details(&source, &page, &mut out).unwrap();
        assert_eq!(out.claims.len(), 1);
        assert_eq!(out.claims[0].value, json!("2024-06-01"));
        assert!(out.claims[0].evidence.excerpt.contains("June 1, 2024"));
        assert_eq!(super::super::asia::west_date("February 31, 2024"), None);
    }

    #[test]
    fn cohere_private_deployment_and_languages_are_model_scoped() {
        let raw = "<div>command-a-03-2025</div><h2>Description</h2><p>Command A only requires three GPUs (C500s / D600s) to run.</p><h2>What Can Command A Be Used For?</h2><ul><li>Multilingual use cases - The model is trained for languages: Lang A, Lang B. It has been trained to respond.</li></ul>";
        let (source, page, mut out) = fixture("west-cohere-doc", raw);
        west_details(&source, &page, &mut out).unwrap();
        let deployment = out
            .claims
            .iter()
            .find(|claim| claim.property == "deployment_requirements")
            .unwrap();
        assert_eq!(deployment.scope, ClaimScope::Model);
        assert_eq!(deployment.value["gpu_count_reported"], "three");
        let language = out
            .claims
            .iter()
            .find(|claim| claim.property == "language_names")
            .unwrap();
        assert_eq!(language.scope, ClaimScope::Model);
        assert_eq!(language.value, serde_json::json!(["Lang A", "Lang B"]));
    }

    #[test]
    fn microsoft_public_data_cutoff_keeps_month_precision_and_conditions() {
        let (mut source, _, _) = fixture("west-microsoft-card", "");
        source.format = Format::Markdown;
        let page = Page::parse(
            &source,
            b"# Phi-4 Model Card\n\n## Model Summary\n\n| Field | Value |\n|---|---|\n| **Status** | Static model trained on an offline dataset with cutoff dates of October 2025 and earlier for publicly available data |\n",
        )
        .unwrap();
        let mut out = super::super::page::metadata(&source, &page).unwrap();
        west_details(&source, &page, &mut out).unwrap();
        let cutoff = out
            .claims
            .iter()
            .find(|claim| claim.property == "knowledge_cutoff")
            .unwrap();
        assert_eq!(cutoff.value, json!("2025-10"));
        assert!(
            cutoff
                .conditions
                .iter()
                .any(|condition| condition.contains("publicly available"))
        );
        assert!(cutoff.evidence.excerpt.contains("October 2025 and earlier"));
    }
}

#[cfg(test)]
mod detail_tests {
    use super::super::model::DocumentKind;
    use super::*;

    fn run(id: &str, body: &str, format: Format) -> Extraction {
        let source = PageSource {
            id: id.into(),
            vendor: "nvidia".into(),
            kind: DocumentKind::ModelCard,
            url: "https://huggingface.co/nvidia/example/raw/revision/README.md".into(),
            format,
            records: vec!["test:model".into()],
        };
        let page = Page::parse(&source, body.as_bytes()).unwrap();
        let mut out = super::super::page::metadata(&source, &page).unwrap();
        extract(&source, &page, &mut out).unwrap();
        out
    }

    #[test]
    fn qwen_rich_fields_are_read_and_recommendations_keep_mode_conditions() {
        let out = run(
            "qwen-card",
            "# Qwen3-8B\n\n## Model Overview\n\n- Number of Layers: 48\n- Number of Attention Heads (GQA): 40 for Q and 12 for KV\n- Number of Paramaters (Non-Embedding): 7.5B\n\n## Best Practices\n\n- For thinking mode (`enable_thinking=True`), use `Temperature=0.4`, `TopP=0.9`, `TopK=18`, and `MinP=0`.\n- For non-thinking mode (`enable_thinking=False`), use `Temperature=0.8`, `TopP=0.7`, `TopK=16`, and `MinP=0`.\n- **Adequate Output Length**: We recommend using an output length of 24,576 tokens for most queries. For complex benchmarking we suggest setting the max output length to 36,864 tokens.\n",
            Format::Markdown,
        );
        let find = |property: &str| out.claims.iter().find(|c| c.property == property).unwrap();
        assert_eq!(find("layer_count").value, json!(48));
        assert_eq!(
            find("attention_heads").value,
            json!({"query":40,"key_value":12})
        );
        assert_eq!(
            find("non_embedding_parameter_count").value,
            json!(7_500_000_000u64)
        );
        let sampling: Vec<_> = out
            .claims
            .iter()
            .filter(|c| c.property == "recommended_sampling")
            .collect();
        assert_eq!(sampling.len(), 2);
        assert_eq!(sampling[0].value["temperature"], json!(0.4));
        assert_eq!(sampling[1].value["temperature"], json!(0.8));
        assert_ne!(sampling[0].conditions, sampling[1].conditions);
        assert!(
            sampling
                .iter()
                .all(|c| c.scope == ClaimScope::Model && !c.conditions.is_empty())
        );
        let output: Vec<_> = out
            .claims
            .iter()
            .filter(|claim| claim.property == "recommended_output_tokens")
            .collect();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0].value, json!(24_576));
        assert_eq!(output[1].value, json!(36_864));
        assert_ne!(output[0].conditions, output[1].conditions);
        assert!(!out.claims.iter().any(|c| c.property == "max_output_tokens"));
    }

    #[test]
    fn nvidia_training_precision_and_interface_terms_have_distinct_scopes() {
        let out = run(
            "nvidia-card",
            "# NVIDIA-Nemotron-3-Super-120B-A12B-BF16\n\n## Model Summary\n\n| Field | Value |\n|---|---|\n| Architecture | RouteMoE - Cells + Experts + Attention hybrid with Predictions |\n| Minimum GPU Requirement | 4× A100-80GB |\n\n## Model Design\n\nThe Super model is pre-trained using FP6 quantization for efficiency.\n\n## License/Terms of Use\n\n**Governing Download Terms with NIM:** Separate container terms apply.\n",
            Format::Markdown,
        );
        let training = out
            .claims
            .iter()
            .find(|c| c.property == "training_precision")
            .unwrap();
        assert_eq!(training.value, json!("FP6"));
        assert_eq!(training.scope, ClaimScope::Model);
        assert!(!training.conditions.is_empty());
        let terms = out
            .claims
            .iter()
            .find(|c| c.property == "license.interface_terms")
            .unwrap();
        assert!(matches!(terms.scope, ClaimScope::Interface { .. }));
        let components = out
            .claims
            .iter()
            .find(|c| c.property == "architecture_components")
            .unwrap();
        assert_eq!(
            components.value,
            json!(["RouteMoE", "Cells", "Experts", "Attention", "Predictions"])
        );
        let gpu = out
            .claims
            .iter()
            .find(|c| c.property == "deployment.minimum_gpu")
            .unwrap();
        assert_eq!(gpu.value, json!("4× A100-80GB"));
    }

    #[test]
    fn family_passage_preserves_complete_original_text_without_leaf_binding() {
        let original = "We are open-weighting two MoE models: <strong>Model A</strong>,\n  plus a complete second sentence.";
        let out = run(
            "qwen-blog",
            &format!("<article class='post-single'><p>{original}</p></article>"),
            Format::Html,
        );
        assert_eq!(out.passages.len(), 1);
        assert_eq!(
            out.passages[0].text,
            "We are open-weighting two MoE models: Model A,\n  plus a complete second sentence."
        );
        assert!(out.passages[0].records.is_empty());
        assert_eq!(out.passages[0].heading.as_deref(), Some("Introduction"));
        assert!(out.claims.is_empty());
    }
}

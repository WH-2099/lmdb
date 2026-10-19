use anyhow::Result;
use scraper::ElementRef;
use serde_json::{Value, json};

use super::asia::{cells, elements, model_heading, number, scaled, section};
use super::content;
use super::model::{
    Claim, ClaimScope, Extraction, LocatedValue, PageEvidence, PageSource, PassageTopic, Precision,
};
use super::page::{Page, text};

pub fn extract(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    let (vendor, record, url) = match source.id.as_str() {
        "kimi-k3-card" => (
            "moonshot",
            "huggingface:model:moonshotai/Kimi-K3",
            "https://huggingface.co/moonshotai/Kimi-K3/raw/main/README.md",
        ),
        "kimi-k3-blog" => (
            "moonshot",
            "modelsdev:model:moonshotai/kimi-k3",
            "https://www.kimi.ai/blog/kimi-k3",
        ),
        "glm-5-3-card" => (
            "zai",
            "huggingface:model:zai-org/GLM-5.3",
            "https://huggingface.co/zai-org/GLM-5.3/raw/main/README.md",
        ),
        "glm-5-3-detail" => (
            "zai",
            "modelsdev:model:zhipuai/glm-5.3",
            "https://docs.z.ai/guides/llm/glm-5.3.md",
        ),
        "glm-5-3-blog" => {
            out.warnings.push("The release URL returned a JavaScript application shell; no article facts were extracted.".into());
            return Ok(());
        }
        "minimax-m3-card" => (
            "minimax",
            "huggingface:model:MiniMaxAI/MiniMax-M3",
            "https://huggingface.co/MiniMaxAI/MiniMax-M3/raw/main/README.md",
        ),
        "minimax-m3-detail" => (
            "minimax",
            "modelsdev:model:minimax/MiniMax-M3",
            "https://www.minimax.io/models/text/m3",
        ),
        "minimax-m3-release" => (
            "minimax",
            "modelsdev:model:minimax/MiniMax-M3",
            "https://www.minimax.io/blog/minimax-m3",
        ),
        "minimax-m21-release" => (
            "minimax",
            "modelsdev:model:minimax/MiniMax-M2.1",
            "https://www.minimax.io/news/minimax-m21",
        ),
        "baidu-release" => (
            "baidu",
            "modelsdev:model:baidu/ernie-4.5-21b-a3b",
            "https://ernie.baidu.com/blog/posts/ernie4.5/",
        ),
        "baidu-21b-card" => (
            "baidu",
            "modelsdev:model:baidu/ernie-4.5-21b-a3b",
            "https://huggingface.co/baidu/ERNIE-4.5-21B-A3B-PT/raw/main/README.md",
        ),
        _ => return Ok(()),
    };
    if source.vendor != vendor
        || !super::repository::same_document_url(&source.url, url)
        || source.records != [record]
    {
        out.warnings.push("The selected document no longer has its exact vendor, URL and model binding; extraction skipped.".into());
        return Ok(());
    }
    match source.id.as_str() {
        "kimi-k3-card" => kimi_card(source, page, out),
        "kimi-k3-blog" => kimi_blog(source, page, out),
        "glm-5-3-card" => glm_card(source, page, out),
        "glm-5-3-detail" => glm_detail(source, page, out),
        "minimax-m3-card" | "minimax-m3-detail" | "minimax-m3-release" | "minimax-m21-release" => {
            minimax(source, page, out)
        }
        "baidu-21b-card" | "baidu-release" => baidu(source, page, out),
        _ => {}
    }
    Ok(())
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
            conditions: conditions.iter().map(|value| (*value).into()).collect(),
            evidence: evidence.clone(),
        });
    }
}

fn one_prefix<'a>(
    nodes: Vec<ElementRef<'a>>,
    prefix: &str,
    out: &mut Extraction,
) -> Option<ElementRef<'a>> {
    let matches: Vec<_> = nodes
        .into_iter()
        .filter(|node| text(*node).starts_with(prefix))
        .collect();
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        out.warnings.push(format!(
            "Expected one passage starting {prefix:?}; found {}.",
            matches.len()
        ));
        None
    }
}

fn parameter_number(value: &str) -> Option<u64> {
    if let Some(value) = value.strip_suffix('T') {
        let count = value.parse::<f64>().ok()? * 1_000_000_000_000.0;
        (count.is_finite() && count > 0.0 && count < u64::MAX as f64)
            .then_some(count.round() as u64)
    } else {
        scaled(value)
    }
}

fn kimi_card(source: &PageSource, page: &Page, out: &mut Extraction) {
    // The published card identifies the model in its introduction and logo, without an H1.
    let Some(intro) = one_prefix(
        elements(&section(page, "1. Model Introduction"), "p"),
        "Kimi K3 is an open-weight,",
        out,
    ) else {
        return;
    };
    if out.title.is_none()
        && let [logo] = page.select("img[alt='Kimi K3']").as_slice()
    {
        out.title = Some(LocatedValue {
            value: logo.attr("alt").unwrap().into(),
            evidence: page.attribute_evidence(*logo, "alt"),
        });
    }
    content::push(
        out,
        page,
        intro,
        PassageTopic::Overview,
        Some("1. Model Introduction"),
        &source.records,
    );
    for row in elements(&section(page, "2. Model Summary"), "tr") {
        let columns = cells(row);
        let [label, value] = columns.as_slice() else {
            continue;
        };
        let label = text(*label);
        let value = text(*value);
        let numeric = match label.as_str() {
            "Total Parameters" => Some((
                "parameter_count",
                parameter_number(&value),
                Some("parameters"),
                Precision::Approximate,
            )),
            "Activated Parameters" => Some((
                "active_parameter_count",
                parameter_number(&value),
                Some("parameters"),
                Precision::Approximate,
            )),
            "Context Length" => Some((
                "context_window",
                number(&value),
                Some("tokens"),
                Precision::Exact,
            )),
            "Number of Layers" => Some(("layer_count", number(&value), None, Precision::Exact)),
            "Number of Experts" => Some(("expert_count", number(&value), None, Precision::Exact)),
            "Selected Experts per Token" => {
                Some(("experts_per_token", number(&value), None, Precision::Exact))
            }
            _ => None,
        };
        if let Some((property, Some(value), unit, precision)) = numeric {
            detail(
                source,
                out,
                property,
                json!(value),
                unit,
                precision,
                ClaimScope::Model,
                &[],
                page.evidence(row),
            );
        }
        let property = match label.as_str() {
            "Architecture" => Some("architecture_type"),
            "Attention Mechanism" => Some("attention_mechanism"),
            _ => None,
        };
        if let Some(property) = property {
            detail(
                source,
                out,
                property,
                json!(value),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &[],
                page.evidence(row),
            );
        }
    }
    for (heading, selector, prefix, topic) in [
        (
            "Key Features",
            "li",
            "Native Multimodality & Long Context",
            PassageTopic::Capabilities,
        ),
        (
            "4. Native MXFP4 Quantization",
            "p",
            "Kimi K3 applies quantization-aware training",
            PassageTopic::Training,
        ),
        (
            "6. Model Usage",
            "p",
            "Kimi K3 was trained in the preserved thinking history mode.",
            PassageTopic::Usage,
        ),
        (
            "7. License",
            "p",
            "Both the code repository and the model weights",
            PassageTopic::License,
        ),
    ] {
        if let Some(node) = one_prefix(elements(&section(page, heading), selector), prefix, out) {
            content::push(out, page, node, topic, Some(heading), &source.records);
            if heading == "7. License"
                && let [license] = elements(&[node], "a").as_slice()
            {
                detail(
                    source,
                    out,
                    "license",
                    json!(text(*license)),
                    None,
                    Precision::Exact,
                    ClaimScope::Model,
                    &[],
                    page.evidence(*license),
                );
            }
        }
    }
}

fn kimi_blog(source: &PageSource, page: &Page, out: &mut Extraction) {
    if !model_heading(page, "Kimi K3: Open Frontier Intelligence", out) {
        return;
    }
    for (prefix, heading, topic) in [
        (
            "Today, we are introducing Kimi K3",
            "Introduction",
            PassageTopic::Overview,
        ),
        (
            "Kimi K3 is built on Kimi Delta Attention",
            "An Open 3T-Class Model",
            PassageTopic::Overview,
        ),
        (
            "Kimi K3 has strong long-horizon coding performance.",
            "Coding",
            PassageTopic::Capabilities,
        ),
        (
            "Kimi K3 is available today on",
            "Introduction",
            PassageTopic::Availability,
        ),
    ] {
        let nodes = if heading == "Introduction" {
            page.select("main .markdown p")
        } else {
            elements(&section(page, heading), "p")
        };
        let Some(node) = one_prefix(nodes, prefix, out) else {
            continue;
        };
        content::push(out, page, node, topic, Some(heading), &source.records);
        if prefix.starts_with("Today,") {
            let value = text(node);
            if let Some((_, rest)) = value.split_once("Kimi K3 is a ")
                && let Some((count, _)) = rest.split_once("-parameter model")
                && let Some(count) = parameter_number(count)
            {
                detail(
                    source,
                    out,
                    "parameter_count",
                    json!(count),
                    Some("parameters"),
                    Precision::Approximate,
                    ClaimScope::Model,
                    &[],
                    page.evidence(node),
                );
            }
            if let Some((_, rest)) = value.split_once("with native vision capabilities and a ")
                && let Some((count, _)) = rest.split_once("-million-token context window")
                && let Some(count) = scaled(&format!("{count}M"))
            {
                detail(
                    source,
                    out,
                    "context_window",
                    json!(count),
                    Some("tokens"),
                    Precision::Approximate,
                    ClaimScope::Model,
                    &[
                        "Rounded announcement wording; the model card specifies the exact native context.",
                    ],
                    page.evidence(node),
                );
            }
        }
    }
}

fn glm_card(source: &PageSource, page: &Page, out: &mut Extraction) {
    if !model_heading(page, "GLM-5.3", out) {
        return;
    }
    if let Some(node) = one_prefix(page.select("p"), "GLM-5.3 uses the same base model as", out) {
        content::push(
            out,
            page,
            node,
            PassageTopic::Training,
            Some("GLM-5.3"),
            &source.records,
        );
    }
    for node in elements(&section(page, "Note"), "li") {
        let value = text(node);
        let scope = ClaimScope::Interface {
            id: source.url.clone(),
        };
        if value.starts_with("GLM-5.3 supports controlling the thinking budget through") {
            content::push(
                out,
                page,
                node,
                PassageTopic::Usage,
                Some("Note"),
                &source.records,
            );
            let codes = elements(&[node], "code");
            if codes.len() >= 4 && text(codes[0]) == "reasoning_effort" {
                detail(
                    source,
                    out,
                    "reasoning_effort",
                    json!(codes[1..4].iter().map(|n| text(*n)).collect::<Vec<_>>()),
                    None,
                    Precision::Exact,
                    scope.clone(),
                    &["Documented inference parameter."],
                    page.evidence(node),
                );
            }
            if let Some((_, rest)) = value.split_once("It defaults to ")
                && let Some(default) = rest.split_whitespace().next()
            {
                detail(
                    source,
                    out,
                    "reasoning_effort_default",
                    json!(default),
                    None,
                    Precision::Exact,
                    scope,
                    &["Default when the request omits reasoning_effort."],
                    page.evidence(node),
                );
            }
        } else if value.starts_with("In the chat template for GLM-5.3,") {
            content::push(
                out,
                page,
                node,
                PassageTopic::Usage,
                Some("Note"),
                &source.records,
            );
            if let Some((_, rest)) = value.split_once("defaults to ")
                && let Some(default) = rest
                    .split_whitespace()
                    .next()
                    .and_then(|value| value.parse::<bool>().ok())
            {
                detail(
                    source,
                    out,
                    "chat_template.clear_thinking_default",
                    json!(default),
                    None,
                    Precision::Exact,
                    scope,
                    &["Chat template default; the card recommends clear_thinking=true for chat."],
                    page.evidence(node),
                );
            }
        }
    }
    // Footnote context and generation budgets describe evaluations, not model ceilings.
}

fn glm_detail(source: &PageSource, page: &Page, out: &mut Extraction) {
    if !model_heading(page, "GLM-5.3", out) {
        return;
    }
    if let Some(node) = one_prefix(
        elements(&section(page, "Overview"), "p"),
        "GLM-5.3 is Z.ai's latest flagship model",
        out,
    ) {
        content::push(
            out,
            page,
            node,
            PassageTopic::Overview,
            Some("Overview"),
            &source.records,
        );
    }
    let scope = ClaimScope::Interface {
        id: source.url.clone(),
    };
    if let Some(node) = one_prefix(
        elements(&section(page, "Feature Changes"), "p"),
        "GLM-5.3 currently supports",
        out,
    ) {
        content::push(
            out,
            page,
            node,
            PassageTopic::Capabilities,
            Some("Feature Changes"),
            &source.records,
        );
        let value = text(node);
        for (prefix, property) in [
            ("with a ", "context_window"),
            ("maximum output length of ", "max_output_tokens"),
        ] {
            if let Some((_, rest)) = value.split_once(prefix)
                && let Some(count) = scaled(rest)
            {
                detail(
                    source,
                    out,
                    property,
                    json!(count),
                    Some("tokens"),
                    Precision::Approximate,
                    scope.clone(),
                    &[
                        "Hosted GLM-5.3 API; publisher K/M notation does not define an exact integer.",
                    ],
                    page.evidence(node),
                );
            }
        }
        if value.contains("supports text-only inputs") {
            detail(
                source,
                out,
                "input_modalities",
                json!(["text"]),
                None,
                Precision::Exact,
                scope.clone(),
                &[],
                page.evidence(node),
            );
        }
        if value.contains("always operates with reasoning enabled") {
            detail(
                source,
                out,
                "reasoning",
                json!(true),
                None,
                Precision::Exact,
                scope.clone(),
                &["Hosted model does not support disabling reasoning."],
                page.evidence(node),
            );
        }
    }
    for row in elements(&section(page, "Feature Changes"), "tr") {
        let columns = cells(row);
        if let [label, values, default, _] = columns.as_slice()
            && text(*label) == "reasoning_effort"
        {
            let values: Vec<_> = text(*values)
                .split(',')
                .map(|v| v.trim().to_owned())
                .collect();
            detail(
                source,
                out,
                "reasoning_effort",
                json!(values),
                None,
                Precision::Exact,
                scope.clone(),
                &[],
                page.evidence(row),
            );
            detail(
                source,
                out,
                "reasoning_effort_default",
                json!(text(*default)),
                None,
                Precision::Exact,
                scope.clone(),
                &[],
                page.evidence(row),
            );
        }
    }
    for node in elements(&section(page, "Capability Support"), "li") {
        let value = text(node);
        let property = if value.starts_with("Function Calling") {
            Some("tool_call")
        } else if value.starts_with("Structured Output") {
            Some("structured_output")
        } else {
            None
        };
        if let Some(property) = property {
            content::push(
                out,
                page,
                node,
                PassageTopic::Capabilities,
                Some("Capability Support"),
                &source.records,
            );
            detail(
                source,
                out,
                property,
                json!(true),
                None,
                Precision::Exact,
                scope.clone(),
                &["Capability listed by the hosted model documentation."],
                page.evidence(node),
            );
        }
    }
}

#[cfg(test)]
fn configured_source(id: &str) -> PageSource {
    let config: super::model::PageConfig =
        serde_saphyr::from_str(include_str!("../../config/official-pages.yaml")).unwrap();
    config
        .pages
        .into_iter()
        .find(|source| source.id == id)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::page;

    fn parse(id: &str, raw: &str) -> Extraction {
        let source = configured_source(id);
        let page = Page::parse(&source, raw.as_bytes()).unwrap();
        let mut out = page::metadata(&source, &page).unwrap();
        extract(&source, &page, &mut out).unwrap();
        out
    }

    #[test]
    fn model_claims_require_the_configured_vendor_url_and_binding() {
        let raw = "## 1. Model Introduction\n\nKimi K3 is an open-weight, native multimodal model.\n\n## 2. Model Summary\n\n| Property | Value |\n| --- | --- |\n| Context Length | 1048576 |\n";
        assert_eq!(parse("kimi-k3-card", raw).claims.len(), 1);
        for mutation in ["binding", "url", "vendor", "multiple_records"] {
            let mut source = configured_source("kimi-k3-card");
            match mutation {
                "binding" => source.records = vec!["huggingface:model:moonshotai/Kimi-K2.5".into()],
                "url" => source.url = "https://huggingface.co/moonshotai/Kimi-K2.5".into(),
                "vendor" => source.vendor = "zai".into(),
                _ => source
                    .records
                    .push("huggingface:model:moonshotai/Kimi-K2.5".into()),
            }
            let page = Page::parse(&source, raw.as_bytes()).unwrap();
            let mut out = page::metadata(&source, &page).unwrap();
            extract(&source, &page, &mut out).unwrap();
            assert!(out.claims.is_empty(), "{mutation}");
            assert!(out.passages.is_empty(), "{mutation}");
            assert!(
                out.warnings
                    .iter()
                    .any(|warning| warning.contains("model binding"))
            );
        }
    }

    #[test]
    fn kimi_reads_changed_native_values_only_from_the_model_summary() {
        let raw = "## Other Model\n\n| Context Length | 999 |\n| --- | --- |\n\n## 1. Model Introduction\n\nKimi K3 is an open-weight, native multimodal agentic model.\n\n## 2. Model Summary\n\n| Property | Value |\n| --- | --- |\n| Total Parameters | 2.6T |\n| Activated Parameters | 99B |\n| Context Length | 2097152 |\n\n## 3. Evaluation Results\n\n| Context Length | 888 |\n| --- | --- |\n";
        let out = parse("kimi-k3-card", raw);
        assert_eq!(out.claims.len(), 3);
        let context = out
            .claims
            .iter()
            .find(|c| c.property == "context_window")
            .unwrap();
        assert_eq!(context.value, 2097152);
        assert_eq!(context.precision, Precision::Exact);
        assert_eq!(context.scope, ClaimScope::Model);
        assert!(
            out.claims
                .iter()
                .any(|c| c.property == "parameter_count" && c.value == 2_600_000_000_000_u64)
        );
        assert!(
            parse("kimi-k3-card", &raw.replace("Kimi K3 is", "Kimi K2.5 is"))
                .claims
                .is_empty()
        );
    }

    #[test]
    fn kimi_blog_selects_the_architecture_section_despite_a_repeated_prefix() {
        let out = parse(
            "kimi-k3-blog",
            r#"<main><h1>Kimi K3: Open Frontier Intelligence</h1><div class="markdown">
            <p>Today, we are introducing Kimi K3. Kimi K3 is a 3.2T-parameter model with native vision capabilities and a 2-million-token context window.</p>
            <p>Kimi K3 is available today on the Kimi API.</p>
            <h2>An Open 3T-Class Model</h2><p>Kimi K3 is built on Kimi Delta Attention, architecture details.</p>
            <h2>Coding</h2><p>Kimi K3 has strong long-horizon coding performance.</p>
            <h2>Appendix</h2><p>Kimi K3 is built on Kimi Delta Attention, repeated appendix summary.</p>
            </div></main>"#,
        );
        assert!(out.warnings.is_empty());
        assert_eq!(out.passages.len(), 4);
        assert!(
            out.passages
                .iter()
                .all(|p| !p.text.contains("appendix summary"))
        );
        assert!(
            out.claims
                .iter()
                .any(|c| c.property == "parameter_count" && c.value == 3_200_000_000_000_u64)
        );
        assert!(out.claims.iter().any(|c| c.property == "context_window"
            && c.value == 2_000_000_u64
            && c.precision == Precision::Approximate));
    }

    #[test]
    fn glm_hosted_limits_keep_interface_scope_and_ignore_examples() {
        let raw = "# GLM-5.3\n\n## Overview\n\nGLM-5.3 is Z.ai's latest flagship model.\n\n## Feature Changes\n\nGLM-5.3 currently supports text-only inputs, with a 2M-token context window and a maximum output length of 256K tokens.\nGLM-5.3 always operates with reasoning enabled.\n\n| Parameter | Values | Default | Description |\n| --- | --- | --- | --- |\n| `reasoning_effort` | `low`, `high`, `max` | `high` | Reasoning budget |\n\n## Quick Start\n\n```json\n{\"max_tokens\": 4096}\n```\n\n## Footnotes\n\nEvaluation used 32K context and 4K output.\n";
        let out = parse("glm-5-3-detail", raw);
        for (property, expected) in [
            ("context_window", 2_000_000),
            ("max_output_tokens", 256_000),
        ] {
            let claims: Vec<_> = out
                .claims
                .iter()
                .filter(|claim| claim.property == property)
                .collect();
            assert_eq!(claims.len(), 1);
            assert_eq!(claims[0].value, expected);
            assert_eq!(claims[0].precision, Precision::Approximate);
            assert!(matches!(claims[0].scope, ClaimScope::Interface { .. }));
        }
        assert!(
            out.claims
                .iter()
                .any(|c| c.property == "reasoning_effort_default" && c.value == "high")
        );
        assert!(
            parse(
                "glm-5-3-detail",
                &raw.replacen("# GLM-5.3", "# GLM-5.3-Flash", 1)
            )
            .claims
            .is_empty()
        );
    }

    #[test]
    fn glm_card_does_not_promote_benchmark_budgets_to_model_limits() {
        let raw = "# GLM-5.3\n\nGLM-5.3 uses the same base model as GLM-5.2.\n\n## Benchmark\n\n### Note\n\n- GLM-5.3 supports controlling the thinking budget through the `reasoning_effort` parameter, which accepts three levels: `low`, `high`, and `max`. It defaults to `high` if not passed.\n\n## Footnotes\n\n- Benchmark context: 1M tokens; maximum generation length: 163,840 tokens.\n";
        let out = parse("glm-5-3-card", raw);
        assert!(
            out.claims
                .iter()
                .all(|c| !matches!(c.property.as_str(), "context_window" | "max_output_tokens"))
        );
        assert!(
            out.claims
                .iter()
                .any(|c| c.property == "reasoning_effort_default" && c.value == "high")
        );
        assert!(out.passages.iter().all(|p| !p.text.contains("163,840")));
    }
}

fn mb_passages(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    selections: &[(&str, &str, PassageTopic, &str)],
    family: bool,
) {
    for (selector, prefix, topic, heading) in selections {
        if let Some(node) = one_prefix(page.select(selector), prefix, out) {
            content::push(
                out,
                page,
                node,
                *topic,
                Some(heading),
                if family { &[] } else { &source.records },
            );
        }
    }
}

fn minimax(source: &PageSource, page: &Page, out: &mut Extraction) {
    match source.id.as_str() {
        "minimax-m3-card" => {
            let Some(overview) = one_prefix(
                page.select("body > p"),
                "MiniMax-M3 is a native multimodal model with ",
                out,
            ) else {
                return;
            };
            content::push(
                out,
                page,
                overview,
                PassageTopic::Overview,
                None,
                &source.records,
            );
            let value = text(overview);
            if let Some(rest) = value.strip_prefix("MiniMax-M3 is a native multimodal model with ")
                && let Some((context, counts)) = rest.split_once(" context. It has ")
                && let Some((total, active)) = counts.split_once(" parameters and ")
                && let Some(active) = active.strip_suffix(" activated parameters.")
            {
                for (property, value, unit) in [
                    ("context_window", scaled(context), "tokens"),
                    (
                        "parameter_count",
                        scaled(total.trim_start_matches('~')),
                        "parameters",
                    ),
                    (
                        "active_parameter_count",
                        scaled(active.trim_start_matches('~')),
                        "parameters",
                    ),
                ] {
                    if let Some(value) = value {
                        detail(
                            source,
                            out,
                            property,
                            json!(value),
                            Some(unit),
                            Precision::Approximate,
                            ClaimScope::Model,
                            &["Advertised rounded model specification."],
                            page.evidence(overview),
                        );
                    }
                }
            }
            for node in elements(&section(page, "MiniMax Sparse Attention (MSA)"), "p") {
                let value = text(node);
                if value.starts_with("M3 is powered by MiniMax Sparse Attention (MSA)") {
                    detail(
                        source,
                        out,
                        "attention_mechanism",
                        json!("MiniMax Sparse Attention (MSA)"),
                        None,
                        Precision::Exact,
                        ClaimScope::Model,
                        &[],
                        page.evidence(node),
                    );
                    content::push(
                        out,
                        page,
                        node,
                        PassageTopic::Overview,
                        Some("MiniMax Sparse Attention (MSA)"),
                        &source.records,
                    );
                }
            }
            for node in elements(&section(page, "How to Use"), "ul") {
                let modes: Vec<_> = elements(&[node], "li > strong > code")
                    .into_iter()
                    .map(text)
                    .collect();
                if modes == ["enabled", "adaptive", "disabled"] {
                    detail(
                        source,
                        out,
                        "reasoning_modes",
                        json!(modes),
                        None,
                        Precision::Exact,
                        ClaimScope::Interface {
                            id: source.url.clone(),
                        },
                        &["Values of the thinking request parameter."],
                        page.evidence(node),
                    );
                    content::push(
                        out,
                        page,
                        node,
                        PassageTopic::Usage,
                        Some("How to Use"),
                        &source.records,
                    );
                }
            }
            for node in elements(&section(page, "Inference Parameters"), "p") {
                if !text(node)
                    .starts_with("We recommend the following parameters for best performance:")
                {
                    continue;
                }
                let mut values = serde_json::Map::new();
                for code in elements(&[node], "code") {
                    for parameter in text(code).split(',') {
                        if let Some((name, value)) = parameter.trim().split_once('=')
                            && matches!(name.trim(), "temperature" | "top_p")
                            && let Ok(value) = value.trim().parse::<serde_json::Number>()
                        {
                            values.insert(name.trim().into(), Value::Number(value));
                        }
                    }
                }
                if !values.is_empty() {
                    detail(
                        source,
                        out,
                        "recommended_sampling",
                        Value::Object(values),
                        None,
                        Precision::Exact,
                        ClaimScope::Model,
                        &["Publisher recommendation; not a default or maximum."],
                        page.evidence(node),
                    );
                    content::push(
                        out,
                        page,
                        node,
                        PassageTopic::Usage,
                        Some("Inference Parameters"),
                        &source.records,
                    );
                }
            }
        }
        "minimax-m3-release" => {
            if !model_heading(
                page,
                "MiniMax M3: Frontier Coding, 1M Context, Native Multimodality — All in One Model",
                out,
            ) {
                return;
            }
            mb_passages(
                source,
                page,
                out,
                &[
                    (
                        "article p",
                        "M3 reaches frontier-level performance on specialized tasks",
                        PassageTopic::Overview,
                        "MiniMax M3",
                    ),
                    (
                        "article p",
                        "M3 is a model that has undergone mixed-modality training",
                        PassageTopic::Training,
                        "Multimodality",
                    ),
                    (
                        "article p",
                        "M3 supports toggling thinking on or off.",
                        PassageTopic::Usage,
                        "API",
                    ),
                ],
                false,
            );
            if let Some(node) = one_prefix(
                page.select("article p"),
                "M3 reaches frontier-level performance on specialized tasks",
                out,
            ) {
                let value = text(node);
                if let Some((_, rest)) = value.split_once("context windows of up to ")
                    && let Some((count, _)) = rest.split_once(" tokens.")
                    && let Some(count) = scaled(count)
                {
                    detail(
                        source,
                        out,
                        "context_window",
                        json!(count),
                        Some("tokens"),
                        Precision::Approximate,
                        ClaimScope::Model,
                        &["Published upper bound in abbreviated units."],
                        page.evidence(node),
                    );
                }
            }
        }
        "minimax-m3-detail" => {
            if !page
                .select("main h1 img")
                .into_iter()
                .any(|node| node.attr("alt") == Some("MiniMax M3"))
            {
                out.warnings
                    .push("Expected MiniMax M3 model heading image was absent.".into());
                return;
            }
            let Some(node) = one_prefix(
                page.select("main section p"),
                "Powered by the proprietary MiniMax Sparse Attention (MSA) architecture, M3 API supports up to ",
                out,
            ) else {
                return;
            };
            content::push(
                out,
                page,
                node,
                PassageTopic::Usage,
                Some("M3 API context"),
                &source.records,
            );
            let value = text(node);
            if let Some((_, rest)) = value.split_once("M3 API supports up to ")
                && let Some((maximum, rest)) =
                    rest.split_once(" tokens context window with a guaranteed minimum of ")
                && let Some((minimum, _)) = rest.split_once(" tokens.")
            {
                for (property, value, condition) in [
                    (
                        "context_window",
                        scaled(maximum),
                        "API context upper bound; not guaranteed for every request.",
                    ),
                    (
                        "context_window_minimum",
                        scaled(minimum),
                        "Guaranteed minimum API context capacity.",
                    ),
                ] {
                    if let Some(value) = value {
                        detail(
                            source,
                            out,
                            property,
                            json!(value),
                            Some("tokens"),
                            Precision::Approximate,
                            ClaimScope::Interface {
                                id: source.url.clone(),
                            },
                            &[condition],
                            page.evidence(node),
                        );
                    }
                }
            }
            mb_passages(
                source,
                page,
                out,
                &[(
                    "main section p",
                    "A natively multimodal model. The entire data pipeline was rebuilt",
                    PassageTopic::Training,
                    "Native multimodality",
                )],
                false,
            );
        }
        "minimax-m21-release" => {
            if !model_heading(
                page,
                "MiniMax M2.1: Significantly Enhanced Multi-Language Programming, Built for Real-World Complex Tasks",
                out,
            ) {
                return;
            }
            mb_passages(
                source,
                page,
                out,
                &[
                    (
                        "main li",
                        "Exceptional Multi-Programming Language Capabilities",
                        PassageTopic::Capabilities,
                        "Programming languages",
                    ),
                    (
                        "main li",
                        "Enhanced Composite Instruction Constraints, Enabling Office Scenarios",
                        PassageTopic::Capabilities,
                        "Office scenarios",
                    ),
                    (
                        "main li",
                        "Outstanding Agent/Tool Scaffolding Generalization Capabilities",
                        PassageTopic::Capabilities,
                        "Agent and tool use",
                    ),
                ],
                false,
            );
        }
        _ => {}
    }
}

fn baidu(source: &PageSource, page: &Page, out: &mut Extraction) {
    if source.id == "baidu-release" {
        if !model_heading(
            page,
            "Announcing the Open Source Release of the ERNIE 4.5 Model Family",
            out,
        ) {
            return;
        }
        mb_passages(
            source,
            page,
            out,
            &[
                (
                    "article .post-content p",
                    "We introduce ERNIE 4.5, a new family",
                    PassageTopic::Overview,
                    "Introduction to ERNIE 4.5",
                ),
                (
                    "article .post-content li",
                    "Multimodal Heterogeneous MoE Pre-Training:",
                    PassageTopic::Training,
                    "ERNIE 4.5 Highlights",
                ),
                (
                    "article .post-content p",
                    "The ERNIE 4.5 models are provided under the Apache License 2.0.",
                    PassageTopic::License,
                    "License",
                ),
            ],
            true,
        );
        return;
    }
    if source.id != "baidu-21b-card" || !model_heading(page, "ERNIE-4.5-21B", out) {
        return;
    }
    for node in elements(&section(page, "Model Overview"), "p") {
        if text(node).starts_with("ERNIE-4.5-21B-A3B is a text MoE Post-trained model,") {
            content::push(
                out,
                page,
                node,
                PassageTopic::Overview,
                Some("Model Overview"),
                &source.records,
            );
        }
    }
    for row in elements(&section(page, "Model Overview"), "tr") {
        let columns = cells(row);
        let [label, value] = columns.as_slice() else {
            continue;
        };
        let label = text(*label);
        let value_text = text(*value);
        let evidence = page.evidence(row);
        match label.as_str() {
            "Params(Total / Activated)" => {
                if let Some((total, active)) = value_text.split_once('/') {
                    for (property, count) in [
                        ("parameter_count", scaled(total)),
                        ("active_parameter_count", scaled(active)),
                    ] {
                        if let Some(count) = count {
                            detail(
                                source,
                                out,
                                property,
                                json!(count),
                                Some("parameters"),
                                Precision::Approximate,
                                ClaimScope::Model,
                                &["Advertised rounded count; active parameters are per token."],
                                evidence.clone(),
                            );
                        }
                    }
                }
            }
            "Context Length" | "Layers" | "Shared Experts" => {
                if let Some(count) = number(&value_text) {
                    let (property, unit) = match label.as_str() {
                        "Context Length" => ("context_window", Some("tokens")),
                        "Layers" => ("layer_count", None),
                        _ => ("shared_expert_count", None),
                    };
                    detail(
                        source,
                        out,
                        property,
                        json!(count),
                        unit,
                        Precision::Exact,
                        ClaimScope::Model,
                        &[],
                        evidence,
                    );
                }
            }
            "Heads(Q/KV)" => {
                if let Some((query, key_value)) = value_text.split_once('/')
                    && let (Some(query), Some(key_value)) =
                        (number(query.trim()), number(key_value.trim()))
                {
                    detail(
                        source,
                        out,
                        "attention_heads",
                        json!({"query":query,"key_value":key_value}),
                        None,
                        Precision::Exact,
                        ClaimScope::Model,
                        &[],
                        evidence,
                    );
                }
            }
            "Modality" if value_text == "Text" => detail(
                source,
                out,
                "input_modalities",
                json!(["text"]),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &[],
                evidence,
            ),
            "Training Stage" => detail(
                source,
                out,
                "training_stages",
                json!([value_text]),
                None,
                Precision::Exact,
                ClaimScope::Model,
                &[],
                evidence,
            ),
            _ => {}
        }
    }
    for node in elements(&section(page, "ERNIE 4.5 Highlights"), "li") {
        if text(node).starts_with("Modality-Specific Post-Training:") {
            content::push(
                out,
                page,
                node,
                PassageTopic::Training,
                Some("ERNIE 4.5 Highlights"),
                &[],
            );
        }
    }
    for node in elements(&section(page, "License"), "p") {
        if text(node).starts_with("The ERNIE 4.5 models are provided under the Apache License 2.0.")
        {
            content::push(out, page, node, PassageTopic::License, Some("License"), &[]);
        }
    }
}

#[cfg(test)]
mod minimax_baidu_tests {
    use super::*;
    use crate::documents::{Format, page};

    fn run(id: &str, format: Format, raw: &str) -> Extraction {
        let source = configured_source(id);
        assert_eq!(source.format, format);
        let page = Page::parse(&source, raw.as_bytes()).unwrap();
        let mut out = page::metadata(&source, &page).unwrap();
        extract(&source, &page, &mut out).unwrap();
        out
    }

    #[test]
    fn baidu_family_figures_and_visual_training_do_not_become_model_claims() {
        let out = run(
            "baidu-release",
            Format::Html,
            r#"<article><h1>Announcing the Open Source Release of the ERNIE 4.5 Model Family</h1><div class="post-content">
            <p>We introduce ERNIE 4.5, a new family with 424B total parameters and 47B active parameters.</p>
            <ul><li>Multimodal Heterogeneous MoE Pre-Training: both text and images.</li></ul>
            <p>The ERNIE 4.5 models are provided under the Apache License 2.0.</p></div></article>"#,
        );
        assert!(out.claims.is_empty());
        assert_eq!(out.passages.len(), 3);
        assert!(
            out.passages
                .iter()
                .all(|passage| passage.records.is_empty())
        );
        let out = run(
            "baidu-21b-card",
            Format::Markdown,
            "# ERNIE-4.5-21B\n\n## ERNIE 4.5 Highlights\n\n1. **Modality-Specific Post-Training:** Family VLMs see images and support thinking.\n\n## Model Overview\n\nERNIE-4.5-21B-A3B is a text MoE Post-trained model, with 21B total parameters.\n\n| Key | Value |\n| --- | --- |\n| Modality | Text |\n| Params(Total / Activated) | 22B / 4B |\n| Context Length | 262144 |\n| Vision Experts(Total / Activated) | 64 / 6 |\n\n## Quickstart\n\n| Key | Value |\n| Context Length | 1024 |\n",
        );
        let claim = |name: &str| {
            out.claims
                .iter()
                .find(|claim| claim.property == name)
                .unwrap()
        };
        assert_eq!(claim("parameter_count").value, json!(22_000_000_000_u64));
        assert_eq!(
            claim("active_parameter_count").value,
            json!(4_000_000_000_u64)
        );
        assert_eq!(claim("context_window").value, json!(262144));
        assert_eq!(claim("context_window").precision, Precision::Exact);
        assert_eq!(claim("input_modalities").value, json!(["text"]));
        assert!(out.claims.iter().all(|claim| claim.property != "reasoning"));
        assert!(
            out.passages
                .iter()
                .filter(|passage| passage.topic == PassageTopic::Training)
                .all(|passage| passage.records.is_empty())
        );
    }

    #[test]
    fn minimax_numbers_follow_source_and_api_context_stays_interface_scoped() {
        let out = run(
            "minimax-m3-card",
            Format::Markdown,
            "MiniMax-M3 is a native multimodal model with 2M context. It has ~429B parameters and ~24B activated parameters.\n",
        );
        assert_eq!(out.claims.len(), 3);
        assert_eq!(
            out.claims
                .iter()
                .find(|claim| claim.property == "context_window")
                .unwrap()
                .value,
            json!(2_000_000)
        );
        assert_eq!(
            out.claims
                .iter()
                .find(|claim| claim.property == "parameter_count")
                .unwrap()
                .value,
            json!(429_000_000_000_u64)
        );
        assert!(
            out.claims
                .iter()
                .all(|claim| claim.precision == Precision::Approximate)
        );
        let out = run(
            "minimax-m3-detail",
            Format::Html,
            r#"<main><h1><img alt="MiniMax M3"></h1><section><p>Powered by the proprietary MiniMax Sparse Attention (MSA) architecture, M3 API supports up to 2M tokens context window with a guaranteed minimum of 768K tokens.</p><p>A natively multimodal model. The entire data pipeline was rebuilt for training.</p></section></main>"#,
        );
        assert_eq!(out.claims.len(), 2);
        assert_eq!(
            out.claims
                .iter()
                .find(|claim| claim.property == "context_window_minimum")
                .unwrap()
                .value,
            json!(768_000)
        );
        assert!(
            out.claims
                .iter()
                .all(|claim| matches!(claim.scope, ClaimScope::Interface { .. }))
        );
    }
}

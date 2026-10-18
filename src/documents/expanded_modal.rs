use anyhow::Result;
use scraper::ElementRef;
use serde_json::{Value, json};

use super::content;
use super::page::{Page, text};
use super::west::{count, unique};
use super::{Claim, ClaimScope, Extraction, PageSource, PassageTopic, Precision};

const FLUX: &str = "huggingface:model:black-forest-labs/FLUX.2-dev";
const SD35: &str = "huggingface:model:stabilityai/stable-diffusion-3.5-large";
const AUDIO: &str = "huggingface:model:stabilityai/stable-audio-open-1.0";
const WAN: &str = "huggingface:model:Wan-AI/Wan2.2-T2V-A14B";
const SEEDANCE: &str = "modelsdev:model:bytedance-seed/seedance-2.5";

pub fn extract(source: &PageSource, page: &Page, out: &mut Extraction) -> Result<()> {
    let (vendor, record, url) = match source.id.as_str() {
        "modal-bfl-flux2-card-public" => (
            "bfl",
            FLUX,
            "https://huggingface.co/black-forest-labs/FLUX.2-dev",
        ),
        "modal-bfl-flux2-blog" => ("bfl", FLUX, "https://bfl.ai/blog/flux-2"),
        "modal-stability-sd35-card-public" => (
            "stability",
            SD35,
            "https://huggingface.co/stabilityai/stable-diffusion-3.5-large",
        ),
        "modal-stability-sd35-blog" => (
            "stability",
            SD35,
            "https://stability.ai/news-updates/introducing-stable-diffusion-3-5",
        ),
        "modal-stability-audio-card-public" => (
            "stability",
            AUDIO,
            "https://huggingface.co/stabilityai/stable-audio-open-1.0",
        ),
        "modal-stability-audio-blog" => (
            "stability",
            AUDIO,
            "https://stability.ai/news-updates/stable-audio-open-research-paper",
        ),
        "modal-wan22-card" => (
            "wan",
            WAN,
            "https://huggingface.co/Wan-AI/Wan2.2-T2V-A14B/raw/c8c270b13ee05bfa474194ac9fb07a5868a97cea/README.md",
        ),
        "modal-wan22-blog" => (
            "wan",
            WAN,
            "https://www.alibabacloud.com/blog/alibaba-releases-wan2-2-to-uplift-cinematic-video-production_602413",
        ),
        "modal-seedance25-blog" => (
            "bytedance",
            SEEDANCE,
            "https://seed.bytedance.com/en/blog/one-take-creation-flexible-referencing-introducing-seedance-2-5",
        ),
        "modal-seedance25-model" => (
            "bytedance",
            SEEDANCE,
            "https://seed.bytedance.com/en/seedance2_5",
        ),
        _ => return Ok(()),
    };
    if source.vendor != vendor || source.url != url || source.records != [record] {
        out.warnings.push("The selected document no longer has its exact vendor, URL and model binding; extraction skipped.".into());
        return Ok(());
    }
    match source.id.as_str() {
        "modal-bfl-flux2-card-public" | "modal-bfl-flux2-blog" => flux(source, page, out),
        "modal-stability-sd35-card-public" | "modal-stability-sd35-blog" => sd35(source, page, out),
        "modal-stability-audio-card-public" | "modal-stability-audio-blog" => {
            audio(source, page, out)
        }
        "modal-wan22-card" | "modal-wan22-blog" => wan(source, page, out),
        "modal-seedance25-blog" | "modal-seedance25-model" => seedance(source, page, out),
        _ => unreachable!(),
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

fn between<'a>(value: &'a str, start: &str, end: &str) -> Option<&'a str> {
    value
        .split_once(start)?
        .1
        .split_once(end)
        .map(|(v, _)| v.trim())
}

fn passage(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    node: ElementRef<'_>,
    topic: PassageTopic,
    model: bool,
) {
    content::push(
        out,
        page,
        node,
        topic,
        None,
        if model { &source.records } else { &[] },
    );
}

type Field<'a> = (&'a str, Value, Option<&'a str>, Precision, &'a [&'a str]);

fn add(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    node: ElementRef<'_>,
    field: Field<'_>,
) {
    let (property, value, unit, precision, conditions) = field;
    out.claims.push(Claim {
        record: source.records[0].clone(),
        property: property.into(),
        value,
        unit: unit.map(str::to_owned),
        precision,
        scope: ClaimScope::Model,
        conditions: conditions.iter().map(|s| (*s).into()).collect(),
        evidence: page.evidence(node),
    });
}

#[allow(clippy::too_many_arguments)]
fn number(
    source: &PageSource,
    page: &Page,
    out: &mut Extraction,
    node: ElementRef<'_>,
    property: &str,
    raw: Option<&str>,
    unit: &str,
    conditions: &[&str],
) {
    if let Some((value, precision)) = raw.and_then(count) {
        add(
            source,
            page,
            out,
            node,
            (property, json!(value), Some(unit), precision, conditions),
        );
    }
}

fn flux(source: &PageSource, page: &Page, out: &mut Extraction) {
    use Precision::Exact;
    let card = source.id.ends_with("card-public");
    let prefix = if card {
        "FLUX.2 [dev] is a "
    } else {
        "FLUX.2 [dev]: "
    };
    let Some(node) = find(page, if card { "p" } else { "li" }, prefix) else {
        return;
    };
    passage(source, page, out, node, PassageTopic::Overview, true);
    let value = text(node);
    let raw = if card {
        between(&value, "is a ", " billion parameter").map(|v| format!("{v}B"))
    } else {
        between(&value, prefix, " open-weight model").map(str::to_owned)
    };
    number(
        source,
        page,
        out,
        node,
        "parameter_count",
        raw.as_deref(),
        "parameters",
        &[],
    );
    if card && value.contains("rectified flow transformer") {
        add(
            source,
            page,
            out,
            node,
            (
                "architecture_type",
                json!("rectified flow transformer"),
                None,
                Exact,
                &[],
            ),
        );
    }
    if card && value.contains("generating, editing and combining images based on text instructions")
    {
        for (property, value) in [
            ("input_modalities", json!(["text", "image"])),
            ("output_modalities", json!(["image"])),
        ] {
            add(source, page, out, node, (property, value, None, Exact, &[]));
        }
    }
    if card {
        if let Some(node) = find(page, "li", "Trained using guidance distillation,") {
            passage(source, page, out, node, PassageTopic::Training, true);
            add(
                source,
                page,
                out,
                node,
                (
                    "training_methods",
                    json!(["guidance distillation"]),
                    None,
                    Exact,
                    &[],
                ),
            );
        }
    } else {
        if value.contains("text-to-image synthesis and image editing with multiple input images") {
            add(
                source,
                page,
                out,
                node,
                (
                    "tasks",
                    json!(["text-to-image", "image editing with multiple input images"]),
                    None,
                    Exact,
                    &[],
                ),
            );
        }
        for prefix in [
            "FLUX.2 is designed for real-world",
            "FLUX.2 now provides multi-reference support",
        ] {
            if let Some(node) = find(page, "p", prefix) {
                passage(source, page, out, node, PassageTopic::Capabilities, false);
            }
        }
    }
}

fn sd35(source: &PageSource, page: &Page, out: &mut Extraction) {
    use Precision::Exact;
    if source.id.ends_with("blog") {
        let Some(node) = find(page, "li > p", "Stable Diffusion 3.5 Large :")
            .or_else(|| find(page, "li > p", "Stable Diffusion 3.5 Large:"))
        else {
            return;
        };
        passage(source, page, out, node, PassageTopic::Capabilities, true);
        let value = text(node);
        let raw = between(&value, "At ", " billion parameters").map(|v| format!("{v}B"));
        number(
            source,
            page,
            out,
            node,
            "parameter_count",
            raw.as_deref(),
            "parameters",
            &[],
        );
        number(
            source,
            page,
            out,
            node,
            "recommended_image_resolution",
            between(
                &value,
                "professional use cases at ",
                " megapixel resolution",
            ),
            "megapixels",
            &["Described as ideal for professional use cases; not a maximum resolution."],
        );
        return;
    }
    if !page
        .select("h1")
        .into_iter()
        .any(|n| text(n) == "Stable Diffusion 3.5 Large")
    {
        return;
    }
    if let Some(node) = find(page, "li", "Model Description: This model generates images") {
        passage(source, page, out, node, PassageTopic::Overview, true);
        let value = text(node);
        if value.contains("Multimodal Diffusion Transformer") {
            add(
                source,
                page,
                out,
                node,
                (
                    "architecture_type",
                    json!("Multimodal Diffusion Transformer (MMDiT)"),
                    None,
                    Exact,
                    &[],
                ),
            );
        }
        if value.contains("three fixed, pretrained text encoders") {
            add(
                source,
                page,
                out,
                node,
                ("text_encoder_count", json!(3), Some("encoders"), Exact, &[]),
            );
        }
        if value.contains("QK-normalization") {
            add(
                source,
                page,
                out,
                node,
                ("qk_normalization", json!(true), None, Exact, &[]),
            );
        }
    }
    if let Some(node) = find(page, "li", "Text Encoders") {
        passage(source, page, out, node, PassageTopic::Training, true);
        let value = text(node);
        let encoders: Vec<_> = ["OpenCLIP-ViT/G", "CLIP-ViT/L", "T5-xxl"]
            .into_iter()
            .filter(|name| value.contains(name))
            .collect();
        if encoders.len() == 3 {
            add(
                source,
                page,
                out,
                node,
                ("text_encoders", json!(encoders), None, Exact, &[]),
            );
        }
        number(
            source,
            page,
            out,
            node,
            "text_encoder_context_length.clips",
            between(&value, "context length ", " tokens"),
            "tokens",
            &["Applies to OpenCLIP-ViT/G and CLIP-ViT/L, not a whole-model context limit."],
        );
        if let Some(raw) = between(
            &value,
            "T5-xxl , context length ",
            " tokens at different stages of training",
        )
        .or_else(|| {
            between(
                &value,
                "T5-xxl, context length ",
                " tokens at different stages of training",
            )
        }) {
            let lengths: Option<Vec<_>> = raw
                .split('/')
                .map(|s| count(s).filter(|(_, p)| *p == Exact).map(|(n, _)| n))
                .collect();
            if let Some(lengths) = lengths.filter(|v| !v.is_empty()) {
                add(
                    source,
                    page,
                    out,
                    node,
                    (
                        "text_encoder_training_context_lengths.t5",
                        json!(lengths),
                        Some("tokens"),
                        Exact,
                        &[
                            "T5-xxl uses different lengths at different training stages; not an inference limit.",
                        ],
                    ),
                );
            }
        }
    }
    if let Some(node) = find(page, "li", "Training Data and Strategy:") {
        let value = text(node);
        if value.contains("synthetic data and filtered publicly available data") {
            passage(source, page, out, node, PassageTopic::Training, true);
            add(
                source,
                page,
                out,
                node,
                (
                    "training_data",
                    json!(["synthetic data", "filtered publicly available data"]),
                    None,
                    Exact,
                    &[],
                ),
            );
        }
    }
}

fn audio(source: &PageSource, page: &Page, out: &mut Extraction) {
    use Precision::Exact;
    let card = source.id.ends_with("card-public");
    if !card
        && !page.select("a[href]").into_iter().any(|n| {
            n.attr("href") == Some("https://huggingface.co/stabilityai/stable-audio-open-1.0")
        })
    {
        return;
    }
    let prefix = if card {
        "Stable Audio Open 1.0 generates variable-length"
    } else {
        "The model generates variable-length stereo audio"
    };
    let Some(node) = find(page, "p", prefix) else {
        return;
    };
    passage(source, page, out, node, PassageTopic::Capabilities, true);
    let value = text(node);
    number(
        source,
        page,
        out,
        node,
        "audio_max_duration",
        between(&value, "up to ", if card { "s)" } else { " seconds" }),
        "seconds",
        &[],
    );
    if let Some(rate) = between(&value, "audio at ", "kHz")
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
    {
        add(
            source,
            page,
            out,
            node,
            ("audio_sample_rate", json!(rate), Some("kHz"), Exact, &[]),
        );
    }
    if value.contains("stereo audio") {
        add(
            source,
            page,
            out,
            node,
            ("audio_channel_layout", json!("stereo"), None, Exact, &[]),
        );
    }
    if !card {
        if let Some(rate) = between(&value, "low latent rate of ", "Hz")
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.0)
        {
            add(
                source,
                page,
                out,
                node,
                (
                    "autoencoder_latent_rate",
                    json!(rate),
                    Some("Hz"),
                    Exact,
                    &[],
                ),
            );
        }
        if value.contains("uses T5 text conditioning instead of CLAP") {
            add(
                source,
                page,
                out,
                node,
                ("text_conditioning", json!("T5"), None, Exact, &[]),
            );
        }
        return;
    }
    if [
        "an autoencoder",
        "T5-based text embedding",
        "transformer-based diffusion (DiT)",
    ]
    .iter()
    .all(|s| value.contains(s))
    {
        add(
            source,
            page,
            out,
            node,
            (
                "architecture_components",
                json!([
                    "autoencoder",
                    "T5-based text embedding",
                    "transformer-based diffusion (DiT)"
                ]),
                None,
                Exact,
                &[],
            ),
        );
    }
    if let Some(node) = find(page, "li", "Language(s)")
        && text(node).split_once(':').is_some_and(|(label, language)| {
            label.trim() == "Language(s)" && language.trim() == "English"
        })
    {
        passage(source, page, out, node, PassageTopic::Capabilities, true);
        add(
            source,
            page,
            out,
            node,
            ("languages", json!(["en"]), None, Exact, &[]),
        );
    }
    if let Some(node) = find(page, "p", "Our dataset consists of ") {
        passage(source, page, out, node, PassageTopic::Training, true);
        let value = text(node);
        number(
            source,
            page,
            out,
            node,
            "training_audio_recordings",
            between(&value, "consists of ", " audio recordings"),
            "recordings",
            &[],
        );
        if let (Some((freesound, Exact)), Some((fma, Exact))) = (
            between(&value, "where ", " are from Freesound").and_then(count),
            between(&value, "Freesound and ", " are from the Free Music Archive").and_then(count),
        ) {
            add(
                source,
                page,
                out,
                node,
                (
                    "training_audio_datasets",
                    json!({"Freesound": freesound, "Free Music Archive": fma}),
                    Some("recordings"),
                    Exact,
                    &[],
                ),
            );
        }
        if value.contains("licensed under CC0, CC BY, or CC Sampling+") {
            add(
                source,
                page,
                out,
                node,
                (
                    "training_data_licenses",
                    json!(["CC0", "CC BY", "CC Sampling+"]),
                    None,
                    Exact,
                    &[],
                ),
            );
        }
        if value.contains("t5-base") {
            add(
                source,
                page,
                out,
                node,
                ("text_encoder", json!("t5-base"), None, Exact, &[]),
            );
        }
    }
}

fn wan(source: &PageSource, page: &Page, out: &mut Extraction) {
    use Precision::Exact;
    let card = source.id.ends_with("card");
    if card {
        let Some(node) = find(page, "p", "This repository contains our T2V-A14B model,") else {
            return;
        };
        passage(source, page, out, node, PassageTopic::Capabilities, true);
        let value = text(node);
        number(
            source,
            page,
            out,
            node,
            "video_generation_duration",
            between(&value, "supports generating ", "s videos"),
            "seconds",
            &["Documented generation duration; source does not call this a maximum."],
        );
        if let Some(resolutions) = between(&value, "at both ", " resolutions") {
            let values: Vec<_> = resolutions.split(" and ").map(str::trim).collect();
            if values.len() == 2
                && values.iter().all(|v| {
                    v.strip_suffix('P')
                        .is_some_and(|n| n.parse::<u32>().is_ok())
                })
            {
                add(
                    source,
                    page,
                    out,
                    node,
                    ("video_resolutions", json!(values), None, Exact, &[]),
                );
            }
        }
        if value.contains("Mixture-of-Experts (MoE)") {
            add(
                source,
                page,
                out,
                node,
                (
                    "architecture_type",
                    json!("Mixture-of-Experts (MoE)"),
                    None,
                    Exact,
                    &[],
                ),
            );
        }
    }
    let prefix = if card {
        "Wan2.2 introduces Mixture-of-Experts (MoE) architecture"
    } else {
        "To address the issue of high computational consumption"
    };
    if let Some(node) = find(page, "p", prefix) {
        let value = text(node);
        let subject = if card {
            "the A14B model series adopts a two-expert design"
        } else {
            "Wan2.2-T2V-A14B and Wan2.2-I2V-A14B implement a two-expert design"
        };
        if value.contains(subject) {
            passage(source, page, out, node, PassageTopic::Overview, !card);
            add(
                source,
                page,
                out,
                node,
                ("expert_count", json!(2), Some("experts"), Exact, &[]),
            );
            let total = between(
                &value,
                "a total of ",
                if card {
                    "B parameters"
                } else {
                    " billion parameters"
                },
            )
            .map(|v| format!("{v}B"));
            let active = between(
                &value,
                "only ",
                if card {
                    "B active parameters"
                } else {
                    " billion parameters are activated"
                },
            )
            .map(|v| format!("{v}B"));
            number(
                source,
                page,
                out,
                node,
                "parameter_count",
                total.as_deref(),
                "parameters",
                &[],
            );
            number(
                source,
                page,
                out,
                node,
                "active_parameter_count",
                active.as_deref(),
                "parameters",
                &["Activated per diffusion denoising step."],
            );
        }
    }
    if !card && let Some(node) = find(page, "p", "Built on the MoE architecture and trained") {
        let value = text(node);
        if value.contains("Wan2.2-T2V-A14B and Wan2.2-I2V-A14B")
            && let Some(controls) = between(&value, "such as ", ", etc.")
        {
            passage(source, page, out, node, PassageTopic::Capabilities, true);
            add(
                source,
                page,
                out,
                node,
                (
                    "generation_controls",
                    json!(controls.split(", ").collect::<Vec<_>>()),
                    None,
                    Exact,
                    &[],
                ),
            );
        }
    }
}

fn seedance(source: &PageSource, page: &Page, out: &mut Extraction) {
    use Precision::Exact;
    if source.id.ends_with("model") {
        let Some(identity) = find(page, "div", "Seedance 2.5 is a next-generation") else {
            return;
        };
        passage(source, page, out, identity, PassageTopic::Overview, true);
        if text(identity).contains("audio-video joint generation model") {
            add(
                source,
                page,
                out,
                identity,
                (
                    "output_modalities",
                    json!(["audio", "video"]),
                    None,
                    Exact,
                    &[],
                ),
            );
        }
        if let Some(node) = find(page, "div", "Create videos up to ") {
            passage(source, page, out, node, PassageTopic::Capabilities, true);
            let value = text(node);
            number(
                source,
                page,
                out,
                node,
                "video_max_duration",
                between(&value, "up to ", " seconds in a single generation"),
                "seconds",
                &["Per single generation."],
            );
            if value.contains("with the option to extend twice") {
                out.claims.push(Claim {
                    record: source.records[0].clone(), property: "video_extension_rounds".into(), value: json!(2), unit: Some("rounds".into()), precision: Exact,
                    scope: ClaimScope::Interface { id: source.url.clone() },
                    conditions: vec!["Option described on public product page; API/version-specific limit is not stated.".into()], evidence: page.evidence(node),
                });
            }
        }
        return;
    }
    let Some(node) = find(page, "p", "Up to ").filter(|n| {
        text(*n).contains("seconds per generation, with multi-round extensions: Seedance 2.5")
    }) else {
        return;
    };
    passage(source, page, out, node, PassageTopic::Capabilities, true);
    let value = text(node);
    number(
        source,
        page,
        out,
        node,
        "video_max_duration",
        between(&value, "Up to ", " seconds per generation"),
        "seconds",
        &["Per single generation; longer videos require extension."],
    );
    if value.contains("supports multiple rounds of extension") {
        add(
            source,
            page,
            out,
            node,
            ("video_extension", json!(true), None, Exact, &[]),
        );
    }
    if let Some(node) = find(page, "p", "Fully upgraded multimodal referencing:") {
        passage(source, page, out, node, PassageTopic::Capabilities, true);
        let value = text(node);
        if value.contains("as reference materials in a single pass") {
            for (property, start, end, unit) in [
                ("reference_image_limit", "up to ", " images", "images"),
                ("reference_video_limit", "images, ", " video clips", "clips"),
                (
                    "reference_audio_limit",
                    "video clips, and ",
                    " audio clips",
                    "clips",
                ),
            ] {
                number(
                    source,
                    page,
                    out,
                    node,
                    property,
                    between(&value, start, end),
                    unit,
                    &["Reference materials supplied in a single pass."],
                );
            }
        }
    }
    if let Some(node) = find(
        page,
        "p",
        "More precise and stable editing capabilities: Seedance 2.5",
    ) {
        let value = text(node);
        if [
            "timestamp-level control",
            "green screen",
            "camera perspective",
            "reference-based editing",
        ]
        .iter()
        .all(|s| value.contains(s))
        {
            passage(source, page, out, node, PassageTopic::Capabilities, true);
            add(
                source,
                page,
                out,
                node,
                (
                    "editing_controls",
                    json!([
                        "timestamp-level audio/video editing",
                        "green screen",
                        "camera perspective",
                        "reference-based editing"
                    ]),
                    None,
                    Exact,
                    &[],
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{DocumentKind, Format, page};

    fn source(id: &str, vendor: &str, record: &str, url: &str) -> PageSource {
        PageSource {
            id: id.into(),
            vendor: vendor.into(),
            kind: DocumentKind::ModelCard,
            url: url.into(),
            format: Format::Html,
            records: vec![record.into()],
        }
    }

    fn run(source: &PageSource, raw: &str) -> Extraction {
        let page = Page::parse(source, raw.as_bytes()).unwrap();
        let mut out = page::metadata(source, &page).unwrap();
        extract(source, &page, &mut out).unwrap();
        for claim in &out.claims {
            let selector = claim.evidence.locator.strip_prefix("css:").unwrap();
            assert_eq!(page.evidence(page.select(selector)[0]), claim.evidence);
        }
        out
    }

    #[test]
    fn family_blog_and_neighbor_variants_do_not_become_dev_claims() {
        let source = source(
            "modal-bfl-flux2-blog",
            "bfl",
            FLUX,
            "https://bfl.ai/blog/flux-2",
        );
        let raw = "<p>FLUX.2 now provides multi-reference support, up to 10 images and 4MP.</p><ul><li>FLUX.2 [pro]: 100B open-weight model.</li><li>FLUX.2 [dev]: 32B open-weight model, combining text-to-image synthesis and image editing with multiple input images.</li></ul>";
        let out = run(&source, raw);
        assert_eq!(out.claims.len(), 2);
        let parameters = out
            .claims
            .iter()
            .find(|c| c.property == "parameter_count")
            .unwrap();
        assert_eq!(parameters.value, json!(32_000_000_000_u64));
        assert_eq!(parameters.precision, Precision::Approximate);
        assert!(
            out.passages
                .iter()
                .any(|p| p.records.is_empty() && p.text.contains("10 images"))
        );
        for mutation in ["binding", "vendor", "url"] {
            let mut wrong = source.clone();
            match mutation {
                "binding" => {
                    wrong.records =
                        vec!["huggingface:model:black-forest-labs/FLUX.2-dev-fp8".into()]
                }
                "vendor" => wrong.vendor = "stability".into(),
                _ => wrong.url = "https://bfl.ai/blog/other".into(),
            }
            assert!(run(&wrong, raw).claims.is_empty());
        }
    }

    #[test]
    fn wan_keeps_5b_frame_rate_out_of_a14b_and_uses_documented_duration() {
        let source = source(
            "modal-wan22-card",
            "wan",
            WAN,
            "https://huggingface.co/Wan-AI/Wan2.2-T2V-A14B/raw/c8c270b13ee05bfa474194ac9fb07a5868a97cea/README.md",
        );
        let out = run(
            &source,
            "<p>The TI2V-5B model supports 720P at 24fps.</p><p>This repository contains our T2V-A14B model, which supports generating 5s videos at both 480P and 720P resolutions. Built with a Mixture-of-Experts (MoE) architecture.</p>",
        );
        assert_eq!(out.claims.len(), 3);
        assert!(
            out.claims
                .iter()
                .all(|c| !c.property.contains("fps") && !c.property.contains("max_duration"))
        );
        let duration = out
            .claims
            .iter()
            .find(|c| c.property == "video_generation_duration")
            .unwrap();
        assert_eq!(duration.value, json!(5));
        assert!(!duration.conditions.is_empty());
    }

    #[test]
    fn seedance_parses_observed_limits_and_retains_interface_scope() {
        let blog = source(
            "modal-seedance25-blog",
            "bytedance",
            SEEDANCE,
            "https://seed.bytedance.com/en/blog/one-take-creation-flexible-referencing-introducing-seedance-2-5",
        );
        let out = run(
            &blog,
            "<p>Up to 31 seconds per generation, with multi-round extensions: Seedance 2.5 supports multiple rounds of extension.</p><p>Fully upgraded multimodal referencing: Users can now input up to 33 images, 11 video clips, and 12 audio clips as reference materials in a single pass.</p>",
        );
        for (name, expected) in [
            ("video_max_duration", 31),
            ("reference_image_limit", 33),
            ("reference_video_limit", 11),
            ("reference_audio_limit", 12),
        ] {
            let claim = out.claims.iter().find(|c| c.property == name).unwrap();
            assert_eq!(claim.value, json!(expected));
            assert!(!claim.conditions.is_empty());
        }
        let model = source(
            "modal-seedance25-model",
            "bytedance",
            SEEDANCE,
            "https://seed.bytedance.com/en/seedance2_5",
        );
        let out = run(
            &model,
            "<div>Seedance 2.5 is a next-generation audio-video joint generation model.</div><div>Create videos up to 30 seconds in a single generation, with the option to extend twice.</div>",
        );
        let extension = out
            .claims
            .iter()
            .find(|c| c.property == "video_extension_rounds")
            .unwrap();
        assert!(matches!(&extension.scope, ClaimScope::Interface { id } if id == &model.url));
    }

    #[test]
    fn stable_audio_separates_output_specifications_from_training_data() {
        let source = source(
            "modal-stability-audio-card-public",
            "stability",
            AUDIO,
            "https://huggingface.co/stabilityai/stable-audio-open-1.0",
        );
        let out = run(
            &source,
            "<p>Stable Audio Open 1.0 generates variable-length (up to 47s) stereo audio at 44.1kHz from text prompts.</p><p>Our dataset consists of 486492 audio recordings, where 472618 are from Freesound and 13874 are from the Free Music Archive (FMA). All audio files are licensed under CC0, CC BY, or CC Sampling+.</p>",
        );
        assert_eq!(
            out.claims
                .iter()
                .find(|c| c.property == "audio_sample_rate")
                .unwrap()
                .unit
                .as_deref(),
            Some("kHz")
        );
        assert_eq!(
            out.claims
                .iter()
                .find(|c| c.property == "audio_max_duration")
                .unwrap()
                .value,
            json!(47)
        );
        assert_eq!(
            out.claims
                .iter()
                .find(|c| c.property == "training_audio_recordings")
                .unwrap()
                .value,
            json!(486492)
        );
        assert!(out.claims.iter().all(|c| c.property != "license"));
    }
}

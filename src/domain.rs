use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use crate::model::{Fact, Record, pointer_segment};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Derived<T> {
    pub value: T,
    pub fact: String,
    pub pointer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DomainMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<TaskLabels>,
    pub modalities: Modalities,
    pub limits: TokenLimits,
    pub parameters: Parameters,
    pub capabilities: Capabilities,
    pub lineage: Lineage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing: Option<PriceSchedule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskLabels {
    pub source: String,
    pub taxonomy: String,
    pub labels: Vec<Derived<TaskLabel>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskLabel {
    pub source_label: String,
    pub family: Option<TaskFamily>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskFamily {
    TextGeneration,
    Embedding,
    Reranking,
    ImageGeneration,
    VideoGeneration,
    SpeechRecognition,
    TextToSpeech,
    Segmentation,
    TimeSeries,
    Robotics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Modalities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Derived<Vec<ModalityLabel>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Derived<Vec<ModalityLabel>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModalityLabel {
    pub source_label: String,
    pub canonical: Option<Modality>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
    Embedding,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TokenLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<Derived<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<Derived<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<Derived<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_provider_context_window: Option<Derived<u64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safetensors_total: Option<Derived<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_dtype: Option<Derived<BTreeMap<String, u64>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Derived<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Derived<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<Derived<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<Derived<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_weights: Option<Derived<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<Derived<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_control: Option<Derived<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_parameters: Option<Derived<Vec<String>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lineage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_models: Option<Derived<Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_model_relation: Option<Derived<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriceSchedule {
    /// Base rates alone are not a quote when conditions or unclassified fields exist.
    pub base: Vec<Derived<PriceRate>>,
    pub conditions: BTreeMap<String, Derived<Value>>,
    pub unclassified: BTreeMap<String, Derived<Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriceRate {
    pub key: String,
    pub component: PriceComponent,
    pub amount: PriceAmount,
    pub basis: Option<PriceBasis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum PriceAmount {
    Known {
        #[schemars(regex(pattern = r"^(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?$"))]
        usd: String,
    },
    Unavailable {
        raw: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriceBasis {
    pub unit: PriceUnit,
    pub quantity: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PriceUnit {
    Token,
    AudioToken,
    ImageToken,
    Image,
    Request,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PriceComponent {
    Input,
    Output,
    CacheRead,
    CacheWrite,
    CacheWriteOneHour,
    Reasoning,
    InputAudio,
    OutputAudio,
    CachedInputAudio,
    InputImage,
    OutputImage,
    ImageToken,
    Request,
    WebSearch,
}

/// Projects known source claims; raw facts remain the authority for unprojected fields.
pub fn project(record: &Record) -> Result<DomainMetadata> {
    let modalities = Modalities {
        input: modalities(record, "input_modalities")?,
        output: modalities(record, "output_modalities")?,
    };
    let pricing = pricing(record, &modalities)?;
    let mut relation: Option<Derived<String>> = typed(record, "base_model_relation")?;
    if record.source == "modelscope"
        && relation
            .as_ref()
            .is_some_and(|value| value.value.is_empty())
    {
        relation = None;
    }
    Ok(DomainMetadata {
        tasks: tasks(record)?,
        modalities,
        limits: TokenLimits {
            context_window: count(record, "context_window", "tokens")?,
            max_input_tokens: count(record, "max_input_tokens", "tokens")?,
            max_output_tokens: count(record, "max_output_tokens", "tokens")?,
            top_provider_context_window: count(record, "top_provider_context_window", "tokens")?,
        },
        parameters: Parameters {
            safetensors_total: count(record, "safetensors_parameter_count", "parameters")?,
            by_dtype: typed(record, "parameters")?,
        },
        capabilities: Capabilities {
            reasoning: typed(record, "reasoning")?,
            thinking: typed(record, "thinking")?,
            tool_call: typed(record, "tool_call")?,
            structured_output: typed(record, "structured_output")?,
            open_weights: typed(record, "open_weights")?,
            attachment: typed(record, "attachment")?,
            temperature_control: if record.source == "modelsdev" {
                typed(record, "temperature")?
            } else {
                None
            },
            supported_parameters: typed(record, "supported_parameters")?,
        },
        lineage: Lineage {
            base_models: string_list(record, "base_models")?,
            base_model_relation: relation,
        },
        pricing,
    })
}

fn evidence<T>(key: &str, fact: &Fact, suffix: &str, value: T) -> Derived<T> {
    Derived {
        value,
        fact: key.into(),
        pointer: format!("{}{suffix}", fact.pointer),
    }
}

fn typed<T: DeserializeOwned>(record: &Record, key: &str) -> Result<Option<Derived<T>>> {
    record
        .facts
        .get(key)
        .map(|fact| {
            let value = serde_json::from_value(fact.value.clone())
                .with_context(|| format!("{}: invalid {key} at {}", record.id, fact.pointer))?;
            Ok(evidence(key, fact, "", value))
        })
        .transpose()
}

fn count(record: &Record, key: &str, unit: &str) -> Result<Option<Derived<u64>>> {
    if let Some(fact) = record.facts.get(key) {
        ensure!(
            fact.unit.as_deref() == Some(unit),
            "{}: {key} must use unit {unit}",
            record.id
        );
    }
    typed(record, key)
}

fn string_list(record: &Record, key: &str) -> Result<Option<Derived<Vec<String>>>> {
    if let Some(fact) = record.facts.get(key)
        && let Some(value) = fact.value.as_str()
    {
        return Ok(Some(evidence(key, fact, "", vec![value.into()])));
    }
    typed(record, key)
}

fn modalities(record: &Record, key: &str) -> Result<Option<Derived<Vec<ModalityLabel>>>> {
    let Some(labels) = typed::<Vec<String>>(record, key)? else {
        return Ok(None);
    };
    Ok(Some(Derived {
        fact: labels.fact,
        pointer: labels.pointer,
        value: labels
            .value
            .into_iter()
            .map(|label| {
                let canonical = match label.as_str() {
                    "text" => Some(Modality::Text),
                    "image" => Some(Modality::Image),
                    "audio" => Some(Modality::Audio),
                    "video" => Some(Modality::Video),
                    "speech" if record.source == "openrouter" => Some(Modality::Audio),
                    "transcription" if record.source == "openrouter" => Some(Modality::Text),
                    "embeddings" if record.source == "openrouter" => Some(Modality::Embedding),
                    _ => None,
                };
                ModalityLabel {
                    source_label: label,
                    canonical,
                }
            })
            .collect(),
    }))
}

fn tasks(record: &Record) -> Result<Option<TaskLabels>> {
    let (taxonomy, labels) = match record.source.as_str() {
        "huggingface" | "modelsdev" => {
            let (key, taxonomy) = if record.source == "huggingface" {
                ("task", "pipeline_tag")
            } else {
                ("type", "type")
            };
            let Some(task) = typed::<String>(record, key)? else {
                return Ok(None);
            };
            (
                taxonomy,
                vec![Derived {
                    value: task_label(&record.source, task.value),
                    fact: task.fact,
                    pointer: task.pointer,
                }],
            )
        }
        "modelscope" => {
            let Some(tasks) = typed::<Vec<Value>>(record, "tasks")? else {
                return Ok(None);
            };
            let mut labels = Vec::new();
            for (index, task) in tasks.value.iter().enumerate() {
                let label = task.get("Name").and_then(Value::as_str).with_context(|| {
                    format!("{}: tasks/{index}/Name must be a string", record.id)
                })?;
                if !label.is_empty() {
                    labels.push(Derived {
                        value: task_label(&record.source, label.into()),
                        fact: tasks.fact.clone(),
                        pointer: format!("{}/{index}/Name", tasks.pointer),
                    });
                }
            }
            ("Tasks.Name", labels)
        }
        _ => return Ok(None),
    };
    Ok(Some(TaskLabels {
        source: record.source.clone(),
        taxonomy: taxonomy.into(),
        labels,
    }))
}

fn task_label(source: &str, label: String) -> TaskLabel {
    let family = match (source, label.as_str()) {
        ("huggingface" | "modelscope", "text-generation") => Some(TaskFamily::TextGeneration),
        ("huggingface" | "modelscope", "text-ranking") | ("modelsdev", "reranking") => {
            Some(TaskFamily::Reranking)
        }
        ("huggingface" | "modelscope", "text-to-speech") => Some(TaskFamily::TextToSpeech),
        ("huggingface" | "modelscope", "image-to-video") => Some(TaskFamily::VideoGeneration),
        ("huggingface", "text-to-image") | ("modelscope", "text-to-image-synthesis") => {
            Some(TaskFamily::ImageGeneration)
        }
        ("huggingface", "text-to-video" | "image-text-to-video")
        | ("modelscope", "text-to-video-synthesis") => Some(TaskFamily::VideoGeneration),
        ("huggingface", "automatic-speech-recognition")
        | ("modelscope", "auto-speech-recognition") => Some(TaskFamily::SpeechRecognition),
        ("huggingface", "image-segmentation" | "mask-generation") => Some(TaskFamily::Segmentation),
        ("huggingface", "time-series-forecasting") => Some(TaskFamily::TimeSeries),
        ("huggingface", "robotics") => Some(TaskFamily::Robotics),
        ("modelscope", "sentence-embedding") | ("modelsdev", "embedding") => {
            Some(TaskFamily::Embedding)
        }
        _ => None,
    };
    TaskLabel {
        source_label: label,
        family,
    }
}

fn pricing(record: &Record, modalities: &Modalities) -> Result<Option<PriceSchedule>> {
    let Some(fact) = record.facts.get("pricing") else {
        return Ok(None);
    };
    let prices = fact
        .value
        .as_object()
        .with_context(|| format!("{}: pricing must be an object", record.id))?;
    let mut schedule = PriceSchedule {
        base: Vec::new(),
        conditions: BTreeMap::new(),
        unclassified: BTreeMap::new(),
    };
    for (key, value) in prices {
        let suffix = format!("/{}", pointer_segment(key));
        if is_condition(&record.source, key) {
            validate_conditions(&record.source, key, value)
                .with_context(|| format!("{}: pricing{suffix}", record.id))?;
            schedule.conditions.insert(
                key.clone(),
                evidence("pricing", fact, &suffix, value.clone()),
            );
        } else if let Some(component) = price_component(&record.source, key) {
            let amount = price_amount(&record.source, value)
                .with_context(|| format!("{}: pricing{suffix}", record.id))?;
            let rate = PriceRate {
                key: key.clone(),
                component,
                amount,
                basis: price_basis(record, modalities, key),
            };
            schedule.base.push(evidence("pricing", fact, &suffix, rate));
        } else {
            schedule.unclassified.insert(
                key.clone(),
                evidence("pricing", fact, &suffix, value.clone()),
            );
        }
    }
    Ok(Some(schedule))
}

fn price_component(source: &str, key: &str) -> Option<PriceComponent> {
    use PriceComponent::*;
    match (source, key) {
        ("modelsdev", "input") | ("openrouter", "prompt") => Some(Input),
        ("modelsdev", "output") | ("openrouter", "completion") => Some(Output),
        ("modelsdev", "cache_read") | ("openrouter", "input_cache_read") => Some(CacheRead),
        ("modelsdev", "cache_write") | ("openrouter", "input_cache_write") => Some(CacheWrite),
        ("modelsdev", "reasoning") | ("openrouter", "internal_reasoning") => Some(Reasoning),
        ("modelsdev", "input_audio") | ("openrouter", "audio") => Some(InputAudio),
        ("modelsdev", "output_audio") | ("openrouter", "audio_output") => Some(OutputAudio),
        ("openrouter", "input_cache_write_1h") => Some(CacheWriteOneHour),
        ("openrouter", "input_audio_cache") => Some(CachedInputAudio),
        ("openrouter", "image") => Some(InputImage),
        ("openrouter", "image_output") => Some(OutputImage),
        ("openrouter", "image_token") => Some(ImageToken),
        ("openrouter", "request") => Some(Request),
        ("openrouter", "web_search") => Some(WebSearch),
        _ => None,
    }
}

fn price_basis(record: &Record, modalities: &Modalities, key: &str) -> Option<PriceBasis> {
    let (unit, quantity) = match record.source.as_str() {
        "modelsdev" => (
            if matches!(key, "input_audio" | "output_audio") {
                PriceUnit::AudioToken
            } else {
                PriceUnit::Token
            },
            1_000_000,
        ),
        "openrouter" => {
            let unit = match key {
                "prompt" | "completion" => {
                    let text_input = modalities.input.as_ref().is_some_and(|input| {
                        input.value.iter().any(|label| label.source_label == "text")
                            && input.value.iter().all(|label| {
                                matches!(label.source_label.as_str(), "text" | "image" | "file")
                            })
                    });
                    let text_output = modalities.output.as_ref().is_some_and(|output| {
                        output.value.len() == 1 && output.value[0].source_label == "text"
                    });
                    if !text_input || !text_output {
                        return None;
                    }
                    PriceUnit::Token
                }
                "audio" | "audio_output" | "input_audio_cache" => PriceUnit::AudioToken,
                "image_token" => PriceUnit::ImageToken,
                "image" | "image_output" => PriceUnit::Image,
                "request" => PriceUnit::Request,
                "web_search" => PriceUnit::Search,
                _ => PriceUnit::Token,
            };
            (unit, 1)
        }
        _ => return None,
    };
    Some(PriceBasis { unit, quantity })
}

fn price_amount(source: &str, value: &Value) -> Result<PriceAmount> {
    let amount = match (source, value) {
        ("modelsdev", Value::Number(value)) => value.to_string(),
        ("openrouter", Value::String(value)) => value.clone(),
        _ => anyhow::bail!("{source} price has the wrong JSON type"),
    };
    if source == "openrouter" && amount == "-1" {
        return Ok(PriceAmount::Unavailable { raw: amount });
    }
    ensure!(
        nonnegative_decimal(&amount),
        "price must be a nonnegative decimal, got {amount}"
    );
    Ok(PriceAmount::Known { usd: amount })
}

fn nonnegative_decimal(value: &str) -> bool {
    let digits = |value: &str| !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit());
    let mut exponent = value.split(['e', 'E']);
    let mut fraction = exponent.next().unwrap_or_default().split('.');
    let integer = fraction.next().unwrap_or_default();
    if !digits(integer) || (integer.len() > 1 && integer.starts_with('0')) {
        return false;
    }
    if fraction.next().is_some_and(|value| !digits(value)) || fraction.next().is_some() {
        return false;
    }
    if exponent.next().is_some_and(|value| {
        !digits(
            value
                .strip_prefix('+')
                .or_else(|| value.strip_prefix('-'))
                .unwrap_or(value),
        )
    }) {
        return false;
    }
    exponent.next().is_none()
}

fn is_condition(source: &str, key: &str) -> bool {
    matches!(
        (source, key),
        ("modelsdev", "tiers" | "context_over_200k") | ("openrouter", "overrides")
    )
}

fn validate_conditions(source: &str, key: &str, value: &Value) -> Result<()> {
    if key == "context_over_200k" {
        return validate_condition_rates(source, value.as_object().context("expected an object")?);
    }
    let items = value.as_array().context("expected an array")?;
    for item in items {
        let object = item
            .as_object()
            .context("condition entry must be an object")?;
        validate_condition_rates(source, object)?;
        if source == "modelsdev" {
            let tier = object
                .get("tier")
                .and_then(Value::as_object)
                .context("tier must be an object")?;
            ensure!(
                tier.get("type").is_some_and(Value::is_string),
                "tier.type must be a string"
            );
            ensure!(
                tier.get("size").and_then(Value::as_u64).is_some(),
                "tier.size must be an unsigned integer"
            );
        } else {
            for key in ["min_prompt_tokens", "utc_start", "utc_end"] {
                if let Some(value) = object.get(key) {
                    ensure!(value.is_number(), "{key} must be a number");
                }
            }
            if let Some(days) = object.get("utc_days") {
                serde_json::from_value::<Vec<String>>(days.clone())
                    .context("utc_days must be a weekday string array")?;
            }
        }
    }
    Ok(())
}

fn validate_condition_rates(source: &str, object: &Map<String, Value>) -> Result<()> {
    for (key, value) in object {
        if price_component(source, key).is_some() {
            price_amount(source, value).with_context(|| format!("invalid {key}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Origin, RecordKind, SCHEMA_VERSION};
    use serde_json::json;

    fn record(source: &str, facts: Value) -> Record {
        Record {
            schema_version: SCHEMA_VERSION,
            kind: RecordKind::Model,
            id: format!("{source}:model:example"),
            source: source.into(),
            upstream_id: "example".into(),
            name: "Example".into(),
            model: None,
            origin: Origin {
                snapshot: "fixture".into(),
                pointer: String::new(),
            },
            facts: facts
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| {
                    let unit = match key.as_str() {
                        "context_window"
                        | "max_input_tokens"
                        | "max_output_tokens"
                        | "top_provider_context_window" => Some("tokens".into()),
                        "safetensors_parameter_count" => Some("parameters".into()),
                        _ => None,
                    };
                    (
                        key.clone(),
                        Fact {
                            value: value.clone(),
                            pointer: format!("/{}", pointer_segment(key)),
                            unit,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn preserves_zero_false_missing_and_open_vocabulary() {
        let record = record(
            "openrouter",
            json!({
                "context_window": 0, "reasoning": false,
                "input_modalities": ["text", "file", "future-input"],
                "output_modalities": ["speech", "transcription", "embeddings", "decisions"],
                "supported_parameters": []
            }),
        );
        let profile = project(&record).unwrap();
        assert_eq!(profile.limits.context_window.unwrap().value, 0);
        assert!(!profile.capabilities.reasoning.unwrap().value);
        assert!(profile.capabilities.tool_call.is_none());
        assert_eq!(
            profile.capabilities.supported_parameters.unwrap().value,
            Vec::<String>::new()
        );
        let output = profile.modalities.output.unwrap();
        assert_eq!(output.pointer, "/output_modalities");
        assert_eq!(
            output.value.iter().map(|v| v.canonical).collect::<Vec<_>>(),
            vec![
                Some(Modality::Audio),
                Some(Modality::Text),
                Some(Modality::Embedding),
                None
            ]
        );
        assert_eq!(output.value[3].source_label, "decisions");
        assert_eq!(profile.modalities.input.unwrap().value[2].canonical, None);
    }

    #[test]
    fn keeps_task_taxonomies_and_normalizes_only_documented_families() {
        let hf = project(&record("huggingface", json!({"task":"text-ranking", "base_models":"lab/base", "parameters":{"BF16":0}, "safetensors_parameter_count":0}))).unwrap();
        let task = &hf.tasks.unwrap().labels[0];
        assert_eq!(task.value.family, Some(TaskFamily::Reranking));
        assert_eq!(task.pointer, "/task");
        assert_eq!(hf.lineage.base_models.unwrap().value, vec!["lab/base"]);
        assert_eq!(hf.parameters.by_dtype.unwrap().value["BF16"], 0);
        let ms = project(&record("modelscope", json!({"tasks":[{"Name":""},{"Name":"sentence-embedding"},{"Name":"future-task"}], "base_model_relation":"", "base_models":[]}))).unwrap();
        let tasks = ms.tasks.unwrap();
        assert_eq!(tasks.source, "modelscope");
        assert_eq!(tasks.taxonomy, "Tasks.Name");
        assert_eq!(tasks.labels.len(), 2);
        assert_eq!(tasks.labels[0].value.family, Some(TaskFamily::Embedding));
        assert_eq!(tasks.labels[0].pointer, "/tasks/1/Name");
        assert_eq!(tasks.labels[1].value.family, None);
        assert_eq!(ms.lineage.base_model_relation, None);
        for task in ["sentence-similarity", "feature-extraction"] {
            assert_eq!(task_label("huggingface", task.into()).family, None);
        }
        for (label, family) in [
            ("embedding", Some(TaskFamily::Embedding)),
            ("reranking", Some(TaskFamily::Reranking)),
            ("decision", None),
            ("video", None),
            ("text-generation", None),
            ("future-type", None),
        ] {
            let md = project(&record("modelsdev", json!({"type":label}))).unwrap();
            let tasks = md.tasks.unwrap();
            assert_eq!(tasks.source, "modelsdev");
            assert_eq!(tasks.taxonomy, "type");
            assert_eq!(tasks.labels.len(), 1);
            assert_eq!(tasks.labels[0].value.source_label, label);
            assert_eq!(tasks.labels[0].value.family, family);
            assert_eq!(tasks.labels[0].fact, "type");
            assert_eq!(tasks.labels[0].pointer, "/type");
        }
        assert!(
            project(&record("modelsdev", json!({})))
                .unwrap()
                .tasks
                .is_none()
        );
    }

    #[test]
    fn rejects_wrong_types_and_units_at_the_projection_boundary() {
        for facts in [
            json!({"reasoning":"false"}),
            json!({"tool_call":null}),
            json!({"context_window":-1}),
            json!({"context_window":1.5}),
            json!({"context_window":"100"}),
            json!({"input_modalities":"text"}),
            json!({"output_modalities":[false]}),
            json!({"parameters":{"BF16":-1}}),
            json!({"safetensors_parameter_count":false}),
            json!({"base_models":[1]}),
            json!({"base_model_relation":[]}),
            json!({"supported_parameters":[null]}),
            json!({"task":[]}),
            json!({"pricing":[]}),
        ] {
            assert!(
                project(&record("huggingface", facts.clone())).is_err(),
                "{facts}"
            );
        }
        assert!(project(&record("modelscope", json!({"tasks":[{"Name":null}]}))).is_err());
        assert!(project(&record("modelsdev", json!({"type":[]}))).is_err());
        let mut invalid = record("modelsdev", json!({"context_window":100}));
        invalid.facts.get_mut("context_window").unwrap().unit = Some("characters".into());
        assert!(project(&invalid).is_err());
        assert!(project(&record("modelsdev", json!({"temperature":0.5}))).is_err());
        assert!(
            project(&record("gemini", json!({"temperature":0.5})))
                .unwrap()
                .capabilities
                .temperature_control
                .is_none()
        );
    }

    #[test]
    fn prices_keep_decimal_precision_and_explicit_denominators() {
        let md = project(&record(
            "modelsdev",
            json!({"pricing":{"input":1.25,"output":0,"input_audio":4}}),
        ))
        .unwrap()
        .pricing
        .unwrap();
        let input = md
            .base
            .iter()
            .find(|rate| rate.value.key == "input")
            .unwrap();
        assert_eq!(
            input.value.amount,
            PriceAmount::Known { usd: "1.25".into() }
        );
        assert_eq!(
            input.value.basis,
            Some(PriceBasis {
                unit: PriceUnit::Token,
                quantity: 1_000_000
            })
        );
        let or = project(&record("openrouter", json!({"input_modalities":["text","image"],"output_modalities":["text"],"pricing":{"prompt":"0.000000000000000000123","completion":"0","future_unit":"0.25","discount":0.1}}))).unwrap().pricing.unwrap();
        let prompt = or
            .base
            .iter()
            .find(|rate| rate.value.key == "prompt")
            .unwrap();
        assert_eq!(
            prompt.value.amount,
            PriceAmount::Known {
                usd: "0.000000000000000000123".into()
            }
        );
        assert_eq!(
            prompt.value.basis,
            Some(PriceBasis {
                unit: PriceUnit::Token,
                quantity: 1
            })
        );
        assert_eq!(prompt.pointer, "/pricing/prompt");
        assert_eq!(or.unclassified["future_unit"].value, "0.25");
        assert_eq!(or.unclassified["discount"].value, 0.1);
    }

    #[test]
    fn does_not_guess_audio_video_or_image_prompt_and_completion_units() {
        for (input, output) in [
            (json!(["text"]), json!(["speech"])),
            (json!(["audio"]), json!(["transcription"])),
            (json!(["text"]), json!(["image"])),
            (json!(["text"]), json!(["video"])),
            (json!(["text", "audio"]), json!(["text"])),
        ] {
            let profile = project(&record("openrouter", json!({"input_modalities":input,"output_modalities":output,"pricing":{"prompt":"0.1","completion":"0.2"}}))).unwrap();
            assert!(
                profile
                    .pricing
                    .unwrap()
                    .base
                    .iter()
                    .all(|rate| rate.value.basis.is_none())
            );
        }
        let profile = project(&record("openrouter", json!({"pricing":{"prompt":"0.1"}}))).unwrap();
        assert!(profile.pricing.unwrap().base[0].value.basis.is_none());
    }

    #[test]
    fn marks_source_sentinel_unavailable_and_rejects_invalid_amounts() {
        let profile = project(&record(
            "openrouter",
            json!({"pricing":{"prompt":"-1","completion":"0"}}),
        ))
        .unwrap();
        let prices = profile.pricing.unwrap();
        assert!(
            prices
                .base
                .iter()
                .any(|rate| rate.value.amount == PriceAmount::Unavailable { raw: "-1".into() })
        );
        for value in [
            json!("-2"),
            json!("NaN"),
            json!("1e--2"),
            json!("01"),
            json!(null),
            json!(0.1),
        ] {
            assert!(project(&record("openrouter", json!({"pricing":{"prompt":value}}))).is_err());
        }
        assert!(project(&record("modelsdev", json!({"pricing":{"input":-1}}))).is_err());
        assert!(project(&record("modelsdev", json!({"pricing":{"input":"1"}}))).is_err());
        for amount in ["0", "1.23", "1e-9", "1E+2"] {
            assert!(nonnegative_decimal(amount));
        }
    }

    #[test]
    fn preserves_ordered_conditions_and_unknown_keys_without_applying_them() {
        let overrides = json!([
            {"min_prompt_tokens":100000,"prompt":"0.2"},
            {"utc_start":2200.0,"utc_end":600.0,"utc_days":["monday","tuesday"],"future_condition":true,"prompt":"0.1"}
        ]);
        let record = record(
            "openrouter",
            json!({"pricing":{"prompt":"0.3","overrides":overrides}}),
        );
        let profile = project(&record).unwrap();
        let schedule = profile.pricing.as_ref().unwrap();
        assert_eq!(schedule.conditions["overrides"].value, overrides);
        assert_eq!(
            schedule.conditions["overrides"].pointer,
            "/pricing/overrides"
        );
        assert_eq!(
            schedule.base[0].value.amount,
            PriceAmount::Known { usd: "0.3".into() }
        );
        let encoded = serde_json::to_value(&profile).unwrap();
        assert_eq!(
            serde_json::from_value::<DomainMetadata>(encoded).unwrap(),
            profile
        );
        let tiers = json!([{"tier":{"type":"context","size":300000},"input":2}]);
        let md = project(&self::record(
            "modelsdev",
            json!({"pricing":{"input":1,"tiers":tiers,"context_over_200k":{"input":2}}}),
        ))
        .unwrap()
        .pricing
        .unwrap();
        assert_eq!(md.conditions["tiers"].value, tiers);
        assert_eq!(md.conditions["context_over_200k"].value, json!({"input":2}));
        assert!(
            project(&self::record(
                "openrouter",
                json!({"pricing":{"overrides":[{"utc_start":"22"}]}})
            ))
            .is_err()
        );
        assert!(
            project(&self::record(
                "modelsdev",
                json!({"pricing":{"tiers":[{"tier":{"type":"context","size":-1}}]}})
            ))
            .is_err()
        );
    }

    #[test]
    fn schema_describes_closed_profiles_and_serialization_rejects_unknown_fields() {
        let schema = serde_json::to_value(schemars::schema_for!(DomainMetadata)).unwrap();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema["properties"].get("pricing").is_some());
        let mut value =
            serde_json::to_value(project(&record("huggingface", json!({}))).unwrap()).unwrap();
        value["capabilities"]["invented"] = json!(true);
        assert!(serde_json::from_value::<DomainMetadata>(value).is_err());
    }
}

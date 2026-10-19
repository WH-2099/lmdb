use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde_json::Value;

use crate::model::{Fact, Origin, Record, RecordKind, SCHEMA_VERSION, Snapshot};

pub fn huggingface(value: &Value, snapshot: &Snapshot) -> Result<Vec<Record>> {
    let id = required_string(value, "id")?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(id);
    Ok(vec![Record {
        schema_version: SCHEMA_VERSION,
        kind: RecordKind::Model,
        id: format!("huggingface:model:{id}"),
        source: "huggingface".into(),
        upstream_id: id.into(),
        name: name.into(),
        model: None,
        origin: Origin {
            snapshot: snapshot.id.clone(),
            pointer: String::new(),
        },
        facts: mapped_facts(
            value,
            "",
            &[
                ("publisher", "/author", None),
                ("task", "/pipeline_tag", None),
                ("library", "/library_name", None),
                ("license", "/cardData/license", None),
                ("license_name", "/cardData/license_name", None),
                ("license_link", "/cardData/license_link", None),
                ("base_models", "/cardData/base_model", None),
                ("base_model_relation", "/cardData/base_model_relation", None),
                ("new_version", "/cardData/new_version", None),
                ("languages", "/cardData/language", None),
                ("datasets", "/cardData/datasets", None),
                ("model_index", "/cardData/model-index", None),
                ("revision", "/sha", None),
                ("architectures", "/config/architectures", None),
                ("model_type", "/config/model_type", None),
                ("torch_dtype", "/config/torch_dtype", None),
                ("dtype", "/config/dtype", None),
                ("quantization_config", "/config/quantization_config", None),
                ("tokenizer_config", "/config/tokenizer_config", None),
                ("auto_map", "/config/auto_map", None),
                ("transformers_version", "/config/transformers_version", None),
                ("transformers_info", "/transformersInfo", None),
                ("num_experts", "/config/num_experts", None),
                ("num_experts_per_tok", "/config/num_experts_per_tok", None),
                ("parameters", "/safetensors/parameters", None),
                (
                    "safetensors_parameter_count",
                    "/safetensors/total",
                    Some("parameters"),
                ),
                ("gated", "/gated", None),
                ("private", "/private", None),
                ("created_at", "/createdAt", None),
                ("updated_at", "/lastModified", None),
                ("tags", "/tags", None),
                ("siblings", "/siblings", None),
                ("eval_results", "/evalResults", None),
            ],
        ),
    }])
}

pub fn modelscope(value: &Value, snapshot: &Snapshot) -> Result<Vec<Record>> {
    ensure!(
        value.get("Code").and_then(Value::as_i64) == Some(200)
            && value.get("Success").and_then(Value::as_bool) == Some(true),
        "ModelScope response must have Code=200 and Success=true"
    );
    let data = value
        .get("Data")
        .context("ModelScope response has no Data")?;
    ensure!(data.is_object(), "ModelScope Data must be an object");
    let namespace = required_string(data, "Path")?;
    let name = required_string(data, "Name")?;
    let id = format!("{namespace}/{name}");
    Ok(vec![Record {
        schema_version: SCHEMA_VERSION,
        kind: RecordKind::Model,
        id: format!("modelscope:model:{id}"),
        source: "modelscope".into(),
        upstream_id: id,
        name: name.into(),
        model: None,
        origin: Origin {
            snapshot: snapshot.id.clone(),
            pointer: "/Data".into(),
        },
        facts: mapped_facts(
            value,
            "/Data",
            &[
                ("publisher", "/Path", None),
                ("tasks", "/Tasks", None),
                ("frameworks", "/Frameworks", None),
                ("license", "/License", None),
                ("license_link", "/LicenseLink", None),
                ("base_models", "/BaseModel", None),
                ("base_model_relation", "/BaseModelRelation", None),
                ("architectures", "/Architectures", None),
                ("revision", "/Revision", None),
                ("created_at", "/CreatedTime", Some("unix_seconds")),
                ("updated_at", "/LastUpdatedTime", Some("unix_seconds")),
                ("visibility", "/Visibility", None),
            ],
        ),
    }])
}

pub fn openrouter(value: &Value, snapshot: &Snapshot) -> Result<Vec<Record>> {
    let models = value
        .get("data")
        .and_then(Value::as_array)
        .context("OpenRouter response data must be an array")?;
    models
        .iter()
        .enumerate()
        .map(|(index, model)| {
            let id = required_string(model, "id")?;
            let pointer = format!("/data/{index}");
            let name = model
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(id);
            Ok(Record {
                schema_version: SCHEMA_VERSION,
                kind: RecordKind::Service,
                id: format!("openrouter:service:{id}"),
                source: "openrouter".into(),
                upstream_id: id.into(),
                name: name.into(),
                model: None,
                origin: Origin {
                    snapshot: snapshot.id.clone(),
                    pointer: pointer.clone(),
                },
                facts: mapped_facts(
                    value,
                    &pointer,
                    &[
                        ("canonical_slug", "/canonical_slug", None),
                        ("hugging_face_id", "/hugging_face_id", None),
                        ("catalog_created_at", "/created", Some("unix_seconds")),
                        ("input_modalities", "/architecture/input_modalities", None),
                        ("output_modalities", "/architecture/output_modalities", None),
                        ("tokenizer", "/architecture/tokenizer", None),
                        ("instruct_type", "/architecture/instruct_type", None),
                        ("context_window", "/context_length", Some("tokens")),
                        (
                            "top_provider_context_window",
                            "/top_provider/context_length",
                            Some("tokens"),
                        ),
                        (
                            "max_output_tokens",
                            "/top_provider/max_completion_tokens",
                            Some("tokens"),
                        ),
                        ("is_moderated", "/top_provider/is_moderated", None),
                        ("supported_parameters", "/supported_parameters", None),
                        ("default_parameters", "/default_parameters", None),
                        ("pricing", "/pricing", None),
                        ("per_request_limits", "/per_request_limits", None),
                        ("supported_voices", "/supported_voices", None),
                        ("knowledge_cutoff", "/knowledge_cutoff", None),
                        ("expiration_date", "/expiration_date", None),
                    ],
                ),
            })
        })
        .collect()
}

pub(super) fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("missing or empty string field {field}"))
}

pub(super) fn mapped_facts(
    value: &Value,
    root: &str,
    fields: &[(&str, &str, Option<&str>)],
) -> BTreeMap<String, Fact> {
    fields
        .iter()
        .filter_map(|(key, suffix, unit)| {
            let pointer = format!("{root}{suffix}");
            let original = value.pointer(&pointer).filter(|value| !value.is_null())?;
            Some((
                (*key).into(),
                Fact {
                    value: original.clone(),
                    pointer,
                    unit: unit.map(str::to_owned),
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot(source: &str) -> Snapshot {
        Snapshot {
            schema_version: SCHEMA_VERSION,
            id: format!("{source}-fixture"),
            source: source.into(),
            url: format!("https://example.test/{source}"),
            retrieved_at: "2026-10-15T00:00:00Z".into(),
            sha256: "0".repeat(64),
            upstream_revision: None,
        }
    }

    #[test]
    fn fixtures_preserve_values_and_absolute_source_pointers() {
        type Adapter = fn(&Value, &Snapshot) -> Result<Vec<Record>>;
        let fixtures: [(&str, &str, Adapter); 3] = [
            (
                "huggingface",
                include_str!("../../tests/fixtures/huggingface.json"),
                huggingface,
            ),
            (
                "modelscope",
                include_str!("../../tests/fixtures/modelscope.json"),
                modelscope,
            ),
            (
                "openrouter",
                include_str!("../../tests/fixtures/openrouter.json"),
                openrouter,
            ),
        ];
        for (source, fixture, adapter) in fixtures {
            let value: Value = serde_json::from_str(fixture).unwrap();
            let snapshot = snapshot(source);
            let records = adapter(&value, &snapshot).unwrap();
            assert!(!records.is_empty());
            for record in records {
                assert_eq!(record.source, source);
                assert_eq!(record.origin.snapshot, snapshot.id);
                assert!(value.pointer(&record.origin.pointer).unwrap().is_object());
                assert!(!record.facts.is_empty());
                for fact in record.facts.values() {
                    assert_eq!(value.pointer(&fact.pointer), Some(&fact.value));
                }
            }
        }
    }

    #[test]
    fn huggingface_keeps_unknown_tasks_and_dtype_parameter_counts() {
        let value: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/huggingface.json")).unwrap();
        let record = huggingface(&value, &snapshot("huggingface"))
            .unwrap()
            .remove(0);
        assert_eq!(record.id, "huggingface:model:example/future-model");
        assert_eq!(record.facts["task"].value, "future-task");
        assert_eq!(record.facts["parameters"].value, json!({"BF16": 0}));
        assert_eq!(record.facts["safetensors_parameter_count"].value, 0);
        assert_eq!(record.facts["gated"].value, false);
        assert!(!record.facts.contains_key("license"));
        let sparse =
            huggingface(&json!({"id": "example/minimal"}), &snapshot("huggingface")).unwrap();
        assert!(sparse[0].facts.is_empty());
        assert_eq!(sparse[0].name, "example/minimal");
    }

    #[test]
    fn huggingface_preserves_config_metadata_and_evaluation_attribution() {
        let value = json!({
            "id": "lab/mixed-weights",
            "sha": "revision-1",
            "gated": "manual",
            "cardData": {
                "license": "other",
                "license_name": "Lab Research License",
                "license_link": "LICENSE.md",
                "language": "future-language",
                "datasets": ["lab/reported-data"],
                "new_version": "lab/successor",
                "model-index": [{
                    "name": "Publisher result",
                    "results": [{"dataset": {"type": "lab/eval"}, "metrics": []}]
                }]
            },
            "config": {
                "torch_dtype": "bfloat16",
                "dtype": "bfloat16",
                "quantization_config": {
                    "quant_method": "fp8", "activation_scheme": "dynamic",
                    "weight_block_size": [128, 128], "modules_to_not_convert": ["lm_head"]
                },
                "tokenizer_config": {"chat_template": "{{ messages }}", "eos_token": "<end>"},
                "auto_map": {"AutoModel": "modeling.Model"},
                "transformers_version": "4.51.0",
                "num_experts": 128,
                "num_experts_per_tok": 8
            },
            "transformersInfo": {"auto_model": "AutoModel", "custom_class": "modeling.Model"},
            "siblings": [
                {"rfilename": "chat_template.jinja"},
                {"rfilename": "model.safetensors", "size": 4000000000u64,
                 "blobId": "git-object-id", "lfs": {"sha256": "weight-content-hash"}}
            ],
            "evalResults": [{
                "filename": "community.yaml", "verified": false,
                "source": {"user": "community-contributor"}, "pullRequest": 42,
                "results": [{"dataset": "lab/eval", "metric": "accuracy", "value": 0}]
            }]
        });
        let record = huggingface(&value, &snapshot("huggingface"))
            .unwrap()
            .remove(0);
        assert_eq!(record.facts["torch_dtype"].value, "bfloat16");
        assert_eq!(
            record.facts["quantization_config"].value["quant_method"],
            "fp8"
        );
        assert_eq!(
            record.facts["model_index"].value,
            value["cardData"]["model-index"]
        );
        assert_eq!(record.facts["eval_results"].value, value["evalResults"]);
        assert_eq!(record.facts["siblings"].value, value["siblings"]);
        for fact in record.facts.values() {
            assert_eq!(value.pointer(&fact.pointer), Some(&fact.value));
        }
        let profile = crate::domain::project(&record).unwrap();
        assert_eq!(profile.licensing.labels.unwrap().value, vec!["other"]);
        let license_name = profile.licensing.name.unwrap();
        assert_eq!(license_name.value, "Lab Research License");
        assert_eq!(license_name.pointer, "/cardData/license_name");
        assert_eq!(profile.licensing.link.unwrap().value, "LICENSE.md");
        assert_eq!(profile.languages.unwrap().value, vec!["future-language"]);
        assert_eq!(profile.lineage.new_version.unwrap().value, "lab/successor");
        assert_eq!(profile.lineage.revision.unwrap().value, "revision-1");
        assert!(profile.capabilities.open_weights.is_none());
        assert!(profile.capabilities.tool_call.is_none());
        assert!(profile.lifecycle.is_none());

        let sparse = huggingface(
            &json!({"id": "lab/sparse", "config": {"quantization_config": null}, "cardData": {"license_name": null}}),
            &snapshot("huggingface"),
        )
        .unwrap()
        .remove(0);
        assert!(sparse.facts.is_empty());
    }

    #[test]
    fn modelscope_uses_namespace_and_name_without_reinterpreting_tasks() {
        let value: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/modelscope.json")).unwrap();
        let record = modelscope(&value, &snapshot("modelscope"))
            .unwrap()
            .remove(0);
        assert_eq!(record.id, "modelscope:model:example/future-model");
        assert_eq!(
            record.facts["tasks"].value,
            json!([{"Name": "future-task"}])
        );
        assert_eq!(record.facts["publisher"].value, "example");
        assert!(!record.facts.contains_key("gated"));
    }

    #[test]
    fn openrouter_preserves_zero_limits_as_service_facts() {
        let value: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/openrouter.json")).unwrap();
        let records = openrouter(&value, &snapshot("openrouter")).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "openrouter:service:example/audio");
        assert_eq!(records[0].kind, RecordKind::Service);
        assert_eq!(records[0].model, None);
        assert_eq!(records[0].facts["context_window"].value, 0);
        assert_eq!(records[0].facts["max_output_tokens"].value, 0);
        assert_eq!(records[0].facts["pricing"].value["completion"], "0");
        assert!(!records[0].facts.contains_key("per_request_limits"));
        assert_eq!(records[1].name, "example/image");
        assert_eq!(
            records[1].facts["context_window"].pointer,
            "/data/1/context_length"
        );
    }

    #[test]
    fn malformed_identities_and_envelopes_fail() {
        for value in [json!({}), json!({"id": null}), json!({"id": " "})] {
            assert!(huggingface(&value, &snapshot("huggingface")).is_err());
        }
        for value in [
            json!({"Code": 403, "Success": true, "Data": {"Path": "x", "Name": "y"}}),
            json!({"Code": 200, "Success": false, "Data": {"Path": "x", "Name": "y"}}),
            json!({"Code": 200, "Data": {"Path": "x", "Name": "y"}}),
            json!({"Code": 200, "Success": true, "Data": []}),
            json!({"Code": 200, "Success": true, "Data": {"Name": "y"}}),
            json!({"Code": 200, "Success": true, "Data": {"Path": "x"}}),
        ] {
            assert!(modelscope(&value, &snapshot("modelscope")).is_err());
        }
        for value in [json!({}), json!({"data": {}}), json!({"data": [{}]})] {
            assert!(openrouter(&value, &snapshot("openrouter")).is_err());
        }
    }
}

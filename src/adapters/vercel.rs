use anyhow::{Context, Result, ensure};
use serde_json::Value;

use super::public::{mapped_facts, required_string};
use crate::model::{Origin, Record, RecordKind, SCHEMA_VERSION, Snapshot};

pub fn models(value: &Value, snapshot: &Snapshot) -> Result<Vec<Record>> {
    ensure!(value.is_object(), "Vercel response must be an object");
    let models = value
        .get("data")
        .and_then(Value::as_array)
        .context("Vercel response data must be an array")?;
    models
        .iter()
        .enumerate()
        .map(|(index, model)| {
            ensure!(model.is_object(), "Vercel model must be an object");
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
                id: format!("vercel:service:{id}"),
                source: "vercel".into(),
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
                        ("owned_by", "/owned_by", None),
                        ("description", "/description", None),
                        ("catalog_created_at", "/created", Some("unix_seconds")),
                        ("catalog_released_at", "/released", Some("unix_seconds")),
                        ("context_window", "/context_window", Some("tokens")),
                        ("max_output_tokens", "/max_tokens", Some("tokens")),
                        ("type", "/type", None),
                        ("input_modalities", "/modalities/input", None),
                        ("output_modalities", "/modalities/output", None),
                        ("supported_parameters", "/supported_parameters", None),
                        ("pricing", "/pricing", None),
                        ("tags", "/tags", None),
                        ("knowledge_cutoff", "/knowledge", None),
                        ("temperature", "/temperature", None),
                        ("reasoning_options", "/reasoning_options", None),
                        ("video_capabilities", "/video_capabilities", None),
                        (
                            "supported_specifications",
                            "/supported_specifications",
                            None,
                        ),
                        ("interleaved", "/interleaved", None),
                        ("regions", "/regions", None),
                        ("zdr", "/zdr", None),
                        ("no_training", "/no_training", None),
                        ("deprecated_at", "/deprecated_at", None),
                    ],
                ),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{self, TaskFamily};
    use serde_json::json;

    fn snapshot() -> Snapshot {
        Snapshot {
            schema_version: SCHEMA_VERSION,
            id: "vercel-fixture".into(),
            source: "vercel".into(),
            url: "https://ai-gateway.vercel.sh/v1/models".into(),
            retrieved_at: "2026-10-19T00:00:00Z".into(),
            sha256: "0".repeat(64),
            upstream_revision: None,
        }
    }

    #[test]
    fn services_preserve_evidence_and_keep_gateway_claims_on_the_service() {
        let value = json!({"object":"list","data":[{
            "id":"lab/language", "name":"Language", "owned_by":"lab", "description":"",
            "created":1755815280u64, "released":1745798400u64,
            "context_window":0, "max_tokens":4096, "type":"language",
            "modalities":{"input":["text","image"],"output":["text"]},
            "supported_parameters":["temperature","tools"], "temperature":false,
            "tags":["reasoning","tool-use","open-weights"], "knowledge":"2025-04",
            "reasoning_options":[{"type":"budget_tokens","min":1}],
            "interleaved":{"field":"reasoning_content"}, "zdr":"all", "no_training":"all",
            "pricing":{"input":"0.00000012","output":"0","future_unit":{"cost":"1"}}
        },{
            "id":"lab/embedding", "name":"", "type":"embedding",
            "modalities":{"input":["text"],"output":["text"]},
            "pricing":{"input":"0.00000001"}, "knowledge":null
        }]});
        let snapshot = snapshot();
        let records = models(&value, &snapshot).unwrap();
        assert_eq!(records.len(), 2);
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record.kind, RecordKind::Service);
            assert_eq!(record.source, "vercel");
            assert!(record.model.is_none());
            assert_eq!(record.origin.snapshot, snapshot.id);
            assert_eq!(record.origin.pointer, format!("/data/{index}"));
            for fact in record.facts.values() {
                assert_eq!(value.pointer(&fact.pointer), Some(&fact.value));
            }
        }
        let record = &records[0];
        assert_eq!(record.id, "vercel:service:lab/language");
        assert_eq!(record.name, "Language");
        assert_eq!(
            record.facts["catalog_released_at"].unit.as_deref(),
            Some("unix_seconds")
        );
        assert_eq!(record.facts["pricing"].unit, None);
        let profile = domain::project(record).unwrap();
        assert_eq!(profile.limits.context_window.unwrap().value, 0);
        assert_eq!(profile.limits.max_output_tokens.unwrap().value, 4096);
        assert_eq!(
            profile.tasks.unwrap().labels[0].value.family,
            Some(TaskFamily::TextGeneration)
        );
        assert!(profile.capabilities.reasoning.is_none());
        assert!(profile.capabilities.tool_call.is_none());
        assert!(profile.capabilities.open_weights.is_none());
        assert!(profile.capabilities.temperature_control.is_none());
        assert_eq!(records[1].name, "lab/embedding");
        assert!(!records[1].facts.contains_key("knowledge_cutoff"));
        assert!(!records[1].facts.contains_key("temperature"));
    }

    #[test]
    fn malformed_envelopes_and_missing_ids_fail() {
        for value in [
            json!([]),
            json!({}),
            json!({"data":{}}),
            json!({"data":[null]}),
            json!({"data":[{}]}),
            json!({"data":[{"id":false}]}),
            json!({"data":[{"id":" "}]}),
        ] {
            assert!(models(&value, &snapshot()).is_err(), "accepted {value}");
        }
        assert!(
            models(&json!({"object":"list","data":[]}), &snapshot())
                .unwrap()
                .is_empty()
        );
    }
}

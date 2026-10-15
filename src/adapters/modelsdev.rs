use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use crate::model::{Fact, Origin, Record, RecordKind, SCHEMA_VERSION, Snapshot, pointer_segment};

const COMMON_FIELDS: &[(&str, &str)] = &[
    ("description", "/description"),
    ("family", "/family"),
    ("type", "/type"),
    ("input_modalities", "/modalities/input"),
    ("output_modalities", "/modalities/output"),
    ("context_window", "/limit/context"),
    ("max_input_tokens", "/limit/input"),
    ("max_output_tokens", "/limit/output"),
    ("reasoning", "/reasoning"),
    ("tool_call", "/tool_call"),
    ("structured_output", "/structured_output"),
    ("open_weights", "/open_weights"),
    ("attachment", "/attachment"),
    ("temperature", "/temperature"),
    ("knowledge_cutoff", "/knowledge"),
    ("release_date", "/release_date"),
    ("last_updated", "/last_updated"),
];

pub fn models(value: &Value, snapshot: &Snapshot) -> Result<Vec<Record>> {
    let entries = value
        .as_object()
        .context("models.dev models response must be an object")?;
    let mut records = Vec::with_capacity(entries.len());
    for (id, value) in entries {
        let name = checked_name(value, id).with_context(|| format!("models.dev model {id}"))?;
        let pointer = format!("/{}", pointer_segment(id));
        let mut facts = BTreeMap::new();
        insert_facts(&mut facts, value, &pointer, COMMON_FIELDS);
        insert_facts(
            &mut facts,
            value,
            &pointer,
            &[
                ("publisher", "/publisher"),
                ("organization", "/organization"),
                ("license", "/license"),
                ("links", "/links"),
                ("weights", "/weights"),
            ],
        );
        records.push(Record {
            schema_version: SCHEMA_VERSION,
            kind: RecordKind::Model,
            id: format!("modelsdev:model:{id}"),
            source: "modelsdev".into(),
            upstream_id: id.clone(),
            name: name.into(),
            model: None,
            origin: Origin {
                snapshot: snapshot.id.clone(),
                pointer,
            },
            facts,
        });
    }
    validate_records(records)
}

pub fn services(value: &Value, snapshot: &Snapshot) -> Result<Vec<Record>> {
    let providers = value
        .get("providers")
        .and_then(Value::as_object)
        .context("models.dev catalog must contain a providers object")?;
    let mut records = Vec::new();
    for (provider_id, provider) in providers {
        checked_name(provider, provider_id)
            .with_context(|| format!("models.dev provider {provider_id}"))?;
        let provider_pointer = format!("/providers/{}", pointer_segment(provider_id));
        let entries = provider
            .get("models")
            .and_then(Value::as_object)
            .with_context(|| format!("models.dev provider {provider_id} must contain models"))?;
        for (model_id, value) in entries {
            let upstream_id = format!("{provider_id}/{model_id}");
            let name = checked_name(value, model_id)
                .with_context(|| format!("models.dev service {upstream_id}"))?;
            let pointer = format!("{provider_pointer}/models/{}", pointer_segment(model_id));
            let model = match value.get("canonical_model_id") {
                None | Some(Value::Null) => None,
                Some(Value::String(id)) if !id.trim().is_empty() => {
                    Some(format!("modelsdev:model:{id}"))
                }
                _ => bail!("models.dev service {upstream_id} has an invalid canonical_model_id"),
            };
            let mut facts = BTreeMap::new();
            insert_facts(&mut facts, value, &pointer, COMMON_FIELDS);
            insert_facts(
                &mut facts,
                value,
                &pointer,
                &[
                    ("canonical_model_id", "/canonical_model_id"),
                    ("status", "/status"),
                    ("reasoning_options", "/reasoning_options"),
                    ("pricing", "/cost"),
                    ("supported_parameters", "/supported_parameters"),
                    ("quantization", "/quantization"),
                ],
            );
            insert_facts(
                &mut facts,
                provider,
                &provider_pointer,
                &[("provider", "/id"), ("provider_name", "/name")],
            );
            records.push(Record {
                schema_version: SCHEMA_VERSION,
                kind: RecordKind::Service,
                id: format!("modelsdev:service:{upstream_id}"),
                source: "modelsdev".into(),
                upstream_id,
                name: name.into(),
                model,
                origin: Origin {
                    snapshot: snapshot.id.clone(),
                    pointer,
                },
                facts,
            });
        }
    }
    validate_records(records)
}

fn checked_name<'a>(value: &'a Value, id: &'a str) -> Result<&'a str> {
    ensure!(value.is_object(), "record must be an object");
    ensure!(!id.trim().is_empty(), "record key must not be empty");
    ensure!(
        value.get("id").and_then(Value::as_str) == Some(id),
        "record id must exist and match its key"
    );
    for field in ["limit", "modalities"] {
        if let Some(value) = value.get(field) {
            ensure!(
                value.is_null() || value.is_object(),
                "{field} must be an object"
            );
        }
    }
    match value.get("name") {
        None | Some(Value::Null) => Ok(id),
        Some(Value::String(name)) if name.trim().is_empty() => Ok(id),
        Some(Value::String(name)) => Ok(name),
        _ => bail!("record name must be a string"),
    }
}

fn insert_facts(
    facts: &mut BTreeMap<String, Fact>,
    value: &Value,
    base: &str,
    fields: &[(&str, &str)],
) {
    for &(key, path) in fields {
        if let Some(value) = value.pointer(path).filter(|value| !value.is_null()) {
            facts.insert(
                key.into(),
                Fact {
                    value: value.clone(),
                    pointer: format!("{base}{path}"),
                    unit: path.starts_with("/limit/").then(|| "tokens".into()),
                },
            );
        }
    }
}

fn validate_records(records: Vec<Record>) -> Result<Vec<Record>> {
    ensure!(
        !records.is_empty(),
        "models.dev response contains no records"
    );
    let mut ids = BTreeSet::new();
    for record in &records {
        ensure!(
            ids.insert(&record.id),
            "duplicate models.dev id: {}",
            record.id
        );
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot() -> Snapshot {
        Snapshot {
            schema_version: SCHEMA_VERSION,
            id: "modelsdev:fixture".into(),
            source: "modelsdev".into(),
            url: "https://models.dev/models.json?type=all".into(),
            retrieved_at: "2026-10-15T00:00:00Z".into(),
            sha256: "0".repeat(64),
            upstream_revision: None,
        }
    }

    fn assert_evidence(value: &Value, records: &[Record]) {
        for record in records {
            let original = value.pointer(&record.origin.pointer).unwrap();
            let id = original["id"].as_str().unwrap();
            match record.kind {
                RecordKind::Model => assert_eq!(id, record.upstream_id),
                RecordKind::Service => {
                    let provider = record.facts["provider"].value.as_str().unwrap();
                    assert_eq!(record.upstream_id, format!("{provider}/{id}"));
                }
            }
            assert_eq!(record.origin.snapshot, "modelsdev:fixture");
            for fact in record.facts.values() {
                assert_eq!(value.pointer(&fact.pointer), Some(&fact.value));
            }
        }
    }

    #[test]
    fn models_preserve_zero_unknown_type_and_evidence() {
        let value: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/modelsdev-models.json"))
                .unwrap();
        let records = models(&value, &snapshot()).unwrap();
        assert_evidence(&value, &records);
        let model = records
            .iter()
            .find(|r| r.upstream_id == "lab~one/vision/one")
            .unwrap();
        assert_eq!(model.id, "modelsdev:model:lab~one/vision/one");
        assert_eq!(model.origin.pointer, "/lab~0one~1vision~1one");
        assert_eq!(model.facts["max_output_tokens"].value, 0);
        assert_eq!(
            model.facts["max_output_tokens"].unit.as_deref(),
            Some("tokens")
        );
        assert_eq!(model.facts["reasoning"].value, false);
        assert!(!model.facts.contains_key("type"));
        assert!(!model.facts.contains_key("structured_output"));
        assert_eq!(model.facts["publisher"].value, "Example Lab");
        let embedding = records
            .iter()
            .find(|r| r.upstream_id == "lab/embedding")
            .unwrap();
        assert_eq!(embedding.name, "lab/embedding");
        assert_eq!(embedding.facts["type"].value, "embedding");
        assert!(!embedding.facts.contains_key("publisher"));
        assert!(records.iter().all(|record| record.model.is_none()));
    }

    #[test]
    fn services_link_only_explicit_ids_and_preserve_provider_pointers() {
        let value: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/modelsdev-catalog.json"))
                .unwrap();
        let records = services(&value, &snapshot()).unwrap();
        assert_evidence(&value, &records);
        let linked = records.iter().find(|r| r.model.is_some()).unwrap();
        assert_eq!(linked.id, "modelsdev:service:host~a/deployed/model~v1");
        assert_eq!(
            linked.model.as_deref(),
            Some("modelsdev:model:lab~one/vision/one")
        );
        assert_eq!(
            linked.origin.pointer,
            "/providers/host~0a/models/deployed~1model~0v1"
        );
        assert_eq!(linked.facts["provider"].pointer, "/providers/host~0a/id");
        assert_eq!(linked.facts["context_window"].value, 16000);
        assert_eq!(linked.facts["max_output_tokens"].value, 0);
        assert_eq!(linked.facts["pricing"].value["input"], 0);
        let unlinked = records.iter().find(|r| r.model.is_none()).unwrap();
        assert_eq!(unlinked.name, "lab/embedding");
        assert!(!unlinked.facts.contains_key("canonical_model_id"));
    }

    #[test]
    fn rejects_invalid_records_and_response_shapes() {
        for value in [
            json!([]),
            json!({}),
            json!({"x": null}),
            json!({"x": {"name": "missing id"}}),
            json!({"x": {"id": "y"}}),
            json!({"": {"id": ""}}),
            json!({"x": {"id": "x", "limit": []}}),
        ] {
            assert!(models(&value, &snapshot()).is_err(), "{value}");
        }
        for value in [
            json!([]),
            json!({}),
            json!({"providers": {}}),
            json!({"providers": []}),
            json!({"providers": {"p": {"id": "p", "models": {}}}}),
            json!({"providers": {"p": {"id": "other", "models": {}}}}),
            json!({"providers": {"p": {"id": "p", "models": []}}}),
            json!({"providers": {"p": {"id": "p", "models": {"m": {"id": "m", "canonical_model_id": ""}}}}}),
        ] {
            assert!(services(&value, &snapshot()).is_err(), "{value}");
        }
    }

    #[test]
    fn rejects_colliding_service_ids() {
        let value = json!({"providers": {
            "a": {"id": "a", "models": {"b/c": {"id": "b/c"}}},
            "a/b": {"id": "a/b", "models": {"c": {"id": "c"}}}
        }});
        assert!(
            services(&value, &snapshot())
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
    }
}

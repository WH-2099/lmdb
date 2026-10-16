use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Fact, Record, RecordKind};
use crate::registry::Registry;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Serves,
    References,
    DerivedFrom,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelationEvidence {
    pub record: String,
    pub fact: String,
    pub snapshot: String,
    /// JSON Pointer to the specific source value, including an array index when needed.
    pub pointer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub from: String,
    pub to: String,
    pub kind: RelationKind,
    pub evidence: Vec<RelationEvidence>,
    /// The explicit source declaration; names and typed tags do not supply this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_derivation: Option<String>,
    pub target_present: bool,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct RelationKey {
    from: String,
    to: String,
    kind: RelationKind,
    declared_derivation: Option<String>,
}

type Edges = BTreeMap<RelationKey, BTreeSet<RelationEvidence>>;

/// Derive only relationships that the source states explicitly.
pub fn derive(registry: &Registry) -> Result<Vec<Relation>> {
    let models = registry
        .records()
        .filter(|record| record.kind == RecordKind::Model)
        .map(|record| record.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut edges = Edges::new();
    for record in registry.records() {
        match (record.source.as_str(), record.kind) {
            ("modelsdev", RecordKind::Service) => {
                if let Some(fact) = record.facts.get("canonical_model_id")
                    && let Some(id) = nonempty_string(record, "canonical_model_id", &fact.value)?
                {
                    let to = format!("modelsdev:model:{id}");
                    ensure!(
                        record.model.as_ref().is_none_or(|model| *model == to),
                        "record {}: model disagrees with canonical_model_id",
                        record.id
                    );
                    add(
                        &mut edges,
                        record,
                        to,
                        RelationKind::Serves,
                        None,
                        [evidence(record, "canonical_model_id", fact, "")],
                    );
                }
            }
            ("modelsdev", RecordKind::Model) => {
                references_from_urls(record, "weights", &mut edges)?;
                references_from_urls(record, "links", &mut edges)?;
            }
            ("openrouter", RecordKind::Service) => {
                if let Some(fact) = record.facts.get("hugging_face_id")
                    && let Some(id) = nonempty_string(record, "hugging_face_id", &fact.value)?
                {
                    add(
                        &mut edges,
                        record,
                        model_id("huggingface", id)?,
                        RelationKind::References,
                        None,
                        [evidence(record, "hugging_face_id", fact, "")],
                    );
                }
            }
            ("huggingface" | "modelscope", RecordKind::Model) => {
                parents(record, &mut edges)?;
            }
            _ => {}
        }
    }
    Ok(edges
        .into_iter()
        .map(|(key, evidence)| Relation {
            target_present: models.contains(key.to.as_str()),
            from: key.from,
            to: key.to,
            kind: key.kind,
            evidence: evidence.into_iter().collect(),
            declared_derivation: key.declared_derivation,
        })
        .collect())
}

fn references_from_urls(record: &Record, key: &str, edges: &mut Edges) -> Result<()> {
    let Some(fact) = record.facts.get(key) else {
        return Ok(());
    };
    let entries = fact
        .value
        .as_array()
        .with_context(|| format!("record {}: {key} must be an array", record.id))?;
    for (index, entry) in entries.iter().enumerate() {
        let entry = entry
            .as_object()
            .with_context(|| format!("record {}: {key}/{index} must be an object", record.id))?;
        if key == "links"
            && !matches!(
                entry.get("type").and_then(Value::as_str),
                Some("model_card" | "weights")
            )
        {
            continue;
        }
        let Some(url) = entry.get("url").and_then(Value::as_str) else {
            continue;
        };
        if let Some(repo) = huggingface_repo_url(url) {
            add(
                edges,
                record,
                format!("huggingface:model:{repo}"),
                RelationKind::References,
                None,
                [evidence(record, key, fact, &format!("/{index}/url"))],
            );
        }
    }
    Ok(())
}

fn parents(record: &Record, edges: &mut Edges) -> Result<()> {
    let Some(fact) = record.facts.get("base_models") else {
        return Ok(());
    };
    let entries = match &fact.value {
        Value::String(_) => vec![(&fact.value, String::new())],
        Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(index, value)| (value, format!("/{index}")))
            .collect(),
        _ => anyhow::bail!(
            "record {}: base_models must be a string or array",
            record.id
        ),
    };
    let declared = record
        .facts
        .get("base_model_relation")
        .map(|fact| nonempty_string(record, "base_model_relation", &fact.value))
        .transpose()?
        .flatten();
    for (value, suffix) in entries {
        let Some(id) = nonempty_string(record, "base_models", value)? else {
            continue;
        };
        let mut proof = vec![evidence(record, "base_models", fact, &suffix)];
        if declared.is_some() {
            proof.push(evidence(
                record,
                "base_model_relation",
                &record.facts["base_model_relation"],
                "",
            ));
        }
        add(
            edges,
            record,
            model_id(&record.source, id)?,
            RelationKind::DerivedFrom,
            declared.map(str::to_owned),
            proof,
        );
    }
    Ok(())
}

fn nonempty_string<'a>(record: &Record, key: &str, value: &'a Value) -> Result<Option<&'a str>> {
    let value = value
        .as_str()
        .with_context(|| format!("record {}: {key} must contain strings", record.id))?;
    if value.is_empty() {
        return Ok(None);
    }
    ensure!(
        value.trim() == value && !value.trim().is_empty(),
        "record {}: {key} has surrounding whitespace",
        record.id
    );
    Ok(Some(value))
}

fn model_id(source: &str, id: &str) -> Result<String> {
    let parts = id.split('/').collect::<Vec<_>>();
    ensure!(
        (1..=2).contains(&parts.len()) && parts.iter().all(|part| repository_segment(part)),
        "invalid {source} model reference: {id}"
    );
    Ok(format!("{source}:model:{id}"))
}

fn repository_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn huggingface_repo_url(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("huggingface.co")
        || !url.username().is_empty()
        || url.password().is_some()
        || value.contains('\\')
    {
        return None;
    }
    // Use the original path so URL normalization cannot hide dot segments or an explicit port.
    let rest = value.strip_prefix("https://huggingface.co/")?;
    let path = rest.split(['?', '#']).next()?;
    if url.path().strip_prefix('/')? != path {
        return None;
    }
    let path = path.strip_suffix('/').unwrap_or(path);
    let parts = path.split('/').collect::<Vec<_>>();
    let (owner, repo) = match parts.as_slice() {
        [owner, repo] => (*owner, *repo),
        [owner, repo, "tree", revision]
            if !revision.is_empty() && *revision != "." && *revision != ".." =>
        {
            (*owner, *repo)
        }
        _ => return None,
    };
    if !repository_segment(owner)
        || !repository_segment(repo)
        || matches!(
            owner,
            "api"
                | "blog"
                | "blob"
                | "collections"
                | "datasets"
                | "docs"
                | "join"
                | "learn"
                | "login"
                | "models"
                | "organizations"
                | "orgs"
                | "papers"
                | "pricing"
                | "resolve"
                | "search"
                | "settings"
                | "spaces"
                | "tasks"
                | "tree"
        )
    {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn evidence(record: &Record, key: &str, fact: &Fact, suffix: &str) -> RelationEvidence {
    RelationEvidence {
        record: record.id.clone(),
        fact: key.to_owned(),
        snapshot: record.origin.snapshot.clone(),
        pointer: format!("{}{suffix}", fact.pointer),
    }
}

fn add(
    edges: &mut Edges,
    record: &Record,
    to: String,
    kind: RelationKind,
    declared_derivation: Option<String>,
    evidence: impl IntoIterator<Item = RelationEvidence>,
) {
    edges
        .entry(RelationKey {
            from: record.id.clone(),
            to,
            kind,
            declared_derivation,
        })
        .or_default()
        .extend(evidence);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Catalog, Origin, SCHEMA_VERSION, pointer_segment};
    use serde_json::json;

    fn record(source: &str, kind: RecordKind, upstream_id: &str, facts: Value) -> Record {
        let kind_name = match kind {
            RecordKind::Model => "model",
            RecordKind::Service => "service",
        };
        let id = format!("{source}:{kind_name}:{upstream_id}");
        let pointer = format!("/records/{}", pointer_segment(&id));
        Record {
            schema_version: SCHEMA_VERSION,
            kind,
            id,
            source: source.to_owned(),
            upstream_id: upstream_id.to_owned(),
            name: upstream_id.to_owned(),
            model: None,
            origin: Origin {
                snapshot: format!("{source}:fixture"),
                pointer: pointer.clone(),
            },
            facts: facts
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        Fact {
                            value: value.clone(),
                            pointer: format!("{pointer}/{}", pointer_segment(key)),
                            unit: None,
                        },
                    )
                })
                .collect(),
        }
    }

    fn registry(records: Vec<Record>) -> Registry {
        Registry {
            catalogs: vec![Catalog {
                schema_version: SCHEMA_VERSION,
                source: "fixture".to_owned(),
                last_sync: None,
                snapshots: Vec::new(),
                records,
            }],
            assertions: Vec::new(),
        }
    }

    fn replay(registry: &Registry, relations: &[Relation]) {
        let mut raw = BTreeMap::<String, Value>::new();
        for record in registry.records() {
            let snapshot = raw
                .entry(record.origin.snapshot.clone())
                .or_insert_with(|| json!({"records": {}}));
            snapshot["records"][&record.id] = Value::Object(
                record
                    .facts
                    .iter()
                    .map(|(key, fact)| (key.clone(), fact.value.clone()))
                    .collect(),
            );
        }
        for relation in relations {
            for proof in &relation.evidence {
                let record = registry.records().find(|r| r.id == proof.record).unwrap();
                assert_eq!(record.origin.snapshot, proof.snapshot);
                let fact = &record.facts[&proof.fact];
                let suffix = proof.pointer.strip_prefix(&fact.pointer).unwrap();
                let value = raw[&proof.snapshot].pointer(&proof.pointer).unwrap();
                assert!(value.is_string());
                assert_eq!(Some(value), fact.value.pointer(suffix));
            }
        }
    }

    #[test]
    fn shared_weights_preserve_aliases_and_quantized_repository() {
        let mut records = ["deepseek-chat", "deepseek-reasoner", "deepseek-v3.2"]
            .into_iter()
            .map(|name| {
                record(
                    "modelsdev",
                    RecordKind::Model,
                    &format!("deepseek/{name}"),
                    json!({"weights": [{"url": "https://huggingface.co/deepseek-ai/DeepSeek-V3.2"}]}),
                )
            })
            .collect::<Vec<_>>();
        records.extend([
            record(
                "modelsdev",
                RecordKind::Model,
                "deepreinforce/ornith-1.0-397b",
                json!({
                    "weights": [
                        {"url": "https://huggingface.co/deepreinforce-ai/Ornith-1.0-397B"},
                        {"url": "https://huggingface.co/deepreinforce-ai/Ornith-1.0-397B-FP8", "quantization": "fp8"}
                    ],
                    "links": [
                        {"type": "model_card", "url": "https://huggingface.co/deepreinforce-ai/Ornith-1.0-397B"},
                        {"type": "license", "url": "https://huggingface.co/some/license"}
                    ]
                }),
            ),
            record("huggingface", RecordKind::Model, "deepseek-ai/DeepSeek-V3.2", json!({})),
        ]);
        let mut registry = registry(records);
        let relations = derive(&registry).unwrap();
        assert_eq!(relations.len(), 5);
        assert!(relations.iter().all(|r| r.kind == RelationKind::References));
        assert_eq!(relations.iter().filter(|r| r.target_present).count(), 3);
        assert!(relations.iter().all(|r| r.declared_derivation.is_none()));
        let plain = relations
            .iter()
            .find(|r| r.to.ends_with("/Ornith-1.0-397B"))
            .unwrap();
        assert_eq!(plain.evidence.len(), 2);
        let fp8 = relations.iter().find(|r| r.to.ends_with("-FP8")).unwrap();
        assert_eq!(
            fp8.evidence[0]
                .pointer
                .rsplit('/')
                .take(2)
                .collect::<Vec<_>>(),
            ["url", "1"]
        );
        replay(&registry, &relations);
        registry.catalogs[0].records.reverse();
        assert_eq!(derive(&registry).unwrap(), relations);
    }

    #[test]
    fn multiple_parents_keep_leaf_evidence_and_source_namespaces() {
        let registry = registry(vec![
            record(
                "huggingface",
                RecordKind::Model,
                "allenai/Molmo2-8B",
                json!({
                    "base_models": ["Qwen/Qwen3-8B", "google/siglip-so400m-patch14-384", "Qwen/Qwen3-8B"],
                    "tags": ["base_model:finetune:Qwen/Qwen3-8B"]
                }),
            ),
            record("huggingface", RecordKind::Model, "Qwen/Qwen3-8B", json!({})),
            record(
                "modelscope",
                RecordKind::Model,
                "Qwen/Qwen3-8B",
                json!({"base_models": ["Qwen/Qwen3-8B-Base"], "base_model_relation": "finetune"}),
            ),
            record(
                "huggingface",
                RecordKind::Model,
                "example/child",
                json!({"base_models": "legacy-parent", "base_model_relation": "future_source_type"}),
            ),
        ]);
        let relations = derive(&registry).unwrap();
        assert_eq!(relations.len(), 4);
        let molmo = relations
            .iter()
            .filter(|r| r.from.ends_with("/Molmo2-8B"))
            .collect::<Vec<_>>();
        assert_eq!(molmo.len(), 2);
        assert!(molmo.iter().all(|r| r.declared_derivation.is_none()));
        assert_eq!(
            molmo
                .iter()
                .find(|r| r.target_present)
                .unwrap()
                .evidence
                .len(),
            2
        );
        let scopes = relations
            .iter()
            .find(|r| r.from.starts_with("modelscope:"))
            .unwrap();
        assert_eq!(scopes.to, "modelscope:model:Qwen/Qwen3-8B-Base");
        assert!(!scopes.target_present);
        assert_eq!(scopes.declared_derivation.as_deref(), Some("finetune"));
        assert!(
            scopes
                .evidence
                .iter()
                .any(|e| e.fact == "base_model_relation")
        );
        assert_eq!(
            relations
                .iter()
                .find(|r| r.to.ends_with(":legacy-parent"))
                .unwrap()
                .declared_derivation
                .as_deref(),
            Some("future_source_type")
        );
        replay(&registry, &relations);
    }

    #[test]
    fn serves_requires_canonical_fact_and_external_references_can_be_missing() {
        let registry = registry(vec![
            record(
                "modelsdev",
                RecordKind::Service,
                "google/gemini",
                json!({"canonical_model_id": "lab~one/vision/one"}),
            ),
            record(
                "openrouter",
                RecordKind::Service,
                "google/gemini",
                json!({"hugging_face_id": "google/missing"}),
            ),
            record(
                "huggingface",
                RecordKind::Model,
                "google/gemini",
                json!({"model": "google/gemini", "tags": ["base_model:google/parent"]}),
            ),
            record(
                "other",
                RecordKind::Service,
                "google/gemini",
                json!({"canonical_model_id": "google/gemini", "hugging_face_id": "google/gemini"}),
            ),
        ]);
        let relations = derive(&registry).unwrap();
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0].kind, RelationKind::Serves);
        assert_eq!(relations[0].to, "modelsdev:model:lab~one/vision/one");
        assert_eq!(relations[1].kind, RelationKind::References);
        assert!(relations.iter().all(|r| !r.target_present));
        replay(&registry, &relations);
    }

    #[test]
    fn only_repository_urls_produce_references() {
        for url in [
            "https://huggingface.co/owner/repo",
            "https://huggingface.co/owner/repo/",
            "https://huggingface.co/owner/repo?download=true#readme",
            "https://huggingface.co/owner/repo/tree/main",
            "https://huggingface.co/owner/repo/tree/abc123/",
        ] {
            assert_eq!(
                huggingface_repo_url(url).as_deref(),
                Some("owner/repo"),
                "{url}"
            );
        }
        for url in [
            "https://huggingface.co/owner",
            "https://huggingface.co/spaces/repo",
            "https://huggingface.co/datasets/repo",
            "https://huggingface.co/organizations/owner",
            "https://huggingface.co/papers/2501.12948",
            "https://huggingface.co/tree/repo",
            "https://huggingface.co/blob/repo",
            "https://huggingface.co/owner/repo/blob/main/LICENSE",
            "https://huggingface.co/owner/repo/resolve/main/model.safetensors",
            "https://huggingface.co/owner/repo/tree/main/subdirectory",
            "https://huggingface.co/owner/repo/tree/",
            "https://huggingface.co/owner/repo/tree/%2e%2e",
            "https://huggingface.co/owner/../repo",
            "https://huggingface.co/owner/repo%2Fother",
            "https://huggingface.co/owner%2frepo/other",
            "https://huggingface.co/owner\\repo",
            "https://huggingface.co:443/owner/repo",
            "https://user@huggingface.co/owner/repo",
            "https://huggingface.co.evil.example/owner/repo",
            "https://other.example/owner/repo",
            "http://huggingface.co/owner/repo",
        ] {
            assert_eq!(huggingface_repo_url(url), None, "{url}");
        }
    }

    #[test]
    fn malformed_explicit_ids_fail_instead_of_becoming_targets() {
        for value in [
            json!("https://huggingface.co/owner/repo"),
            json!(" ../repo"),
            json!(12),
            json!(["owner/repo", 12]),
        ] {
            let registry = registry(vec![record(
                "huggingface",
                RecordKind::Model,
                "owner/child",
                json!({"base_models": value}),
            )]);
            assert!(derive(&registry).is_err());
        }
        let mut service = record(
            "modelsdev",
            RecordKind::Service,
            "provider/service",
            json!({"canonical_model_id": "owner/model"}),
        );
        service.model = Some("modelsdev:model:owner/other".to_owned());
        assert!(derive(&registry(vec![service])).is_err());
    }
}

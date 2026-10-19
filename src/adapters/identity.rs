use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde_json::Value;

use crate::model::{Record, RecordKind, Snapshot};

pub(crate) struct IdentityIndex(BTreeMap<String, Identity>);

struct Identity {
    kind: RecordKind,
    id: String,
    upstream_id: String,
    name: String,
    model: Option<String>,
}

impl IdentityIndex {
    pub(crate) fn from_snapshot(value: &Value, snapshot: &Snapshot) -> Result<Option<Self>> {
        let Some(records) = super::normalize(value, snapshot)? else {
            return Ok(None);
        };
        let mut identities = BTreeMap::new();
        for record in records {
            let identity = Identity {
                kind: record.kind,
                id: record.id,
                upstream_id: record.upstream_id,
                name: record.name,
                model: record.model,
            };
            ensure!(
                identities.insert(record.origin.pointer, identity).is_none(),
                "snapshot {}: multiple identities at one origin pointer",
                snapshot.id
            );
        }
        Ok(Some(Self(identities)))
    }

    pub(crate) fn validate(&self, record: &Record) -> Result<()> {
        let expected = self.0.get(&record.origin.pointer).with_context(|| {
            format!(
                "record {}: origin pointer does not identify a source record",
                record.id
            )
        })?;
        // Only compare identity: older catalogs may contain a subset of today's mapped facts.
        for (field, matches) in [
            ("id", record.id == expected.id),
            ("upstream_id", record.upstream_id == expected.upstream_id),
            ("name", record.name == expected.name),
            ("kind", record.kind == expected.kind),
            ("model", record.model == expected.model),
        ] {
            ensure!(
                matches,
                "record {}: {field} does not match identity at snapshot pointer {}",
                record.id,
                record.origin.pointer
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Catalog, SCHEMA_VERSION},
        registry::Registry,
        store,
    };
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::path::Path;

    fn catalog(cache: &Path, source: &str, path: &str, raw: Value) -> Result<Catalog> {
        let bytes = serde_json::to_vec(&raw)?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let snapshot = Snapshot {
            schema_version: SCHEMA_VERSION,
            id: format!("{source}:{sha256}"),
            source: source.into(),
            url: format!("https://example.com{path}"),
            retrieved_at: "2026-10-19T00:00:00Z".into(),
            sha256,
            upstream_revision: None,
        };
        store::write_bytes(
            &cache
                .join("snapshots")
                .join(format!("{}.json", snapshot.sha256)),
            &bytes,
        )?;
        Ok(Catalog {
            schema_version: SCHEMA_VERSION,
            source: source.into(),
            last_sync: None,
            records: super::super::normalize(&raw, &snapshot)?.unwrap(),
            snapshots: vec![snapshot],
        })
    }

    fn validate(catalog: Catalog, cache: &Path) -> Result<()> {
        Registry {
            catalogs: vec![catalog],
            assertions: vec![],
        }
        .validate(Some(cache))
    }

    #[test]
    fn source_identities_reject_forged_ids_names_and_kinds() -> Result<()> {
        let fixtures = [
            ("huggingface", "/models", json!({"id":"lab/a","name":"A"})),
            (
                "modelscope",
                "/models",
                json!({"Code":200,"Success":true,"Data":{"Path":"lab","Name":"a"}}),
            ),
            (
                "openrouter",
                "/models",
                json!({"data":[{"id":"lab/a","name":"A"}]}),
            ),
            (
                "vercel",
                "/models",
                json!({"data":[{"id":"lab/a","name":"A"}]}),
            ),
            (
                "modelsdev",
                "/models.json",
                json!({"lab/a":{"id":"lab/a","name":"A"}}),
            ),
            (
                "modelsdev",
                "/catalog.json",
                json!({"providers":{"lab":{"id":"lab","models":{"a":{"id":"a","name":"A"}}}}}),
            ),
            (
                "artificialanalysis",
                "/models",
                json!({"data":[{"id":"a","name":"A"}]}),
            ),
            ("openai", "/models", json!({"data":[{"id":"a"}]})),
            (
                "anthropic",
                "/models",
                json!({"data":[{"id":"a","display_name":"A"}]}),
            ),
            (
                "gemini",
                "/models",
                json!({"models":[{"name":"models/a","displayName":"A"}]}),
            ),
            (
                "deepseek",
                "/models",
                json!({"data":[{"id":"a","name":"A"}]}),
            ),
            ("cohere", "/models", json!({"models":[{"name":"a"}]})),
            (
                "fal",
                "/models",
                json!({"models":[{"endpoint_id":"lab/a","metadata":{"display_name":"A"}}]}),
            ),
            (
                "replicate",
                "/models",
                json!({"results":[{"owner":"lab","name":"a"}]}),
            ),
        ];
        for (source, path, raw) in fixtures {
            let cache = tempfile::tempdir()?;
            let original = catalog(cache.path(), source, path, raw)?;
            validate(original.clone(), cache.path())?;
            for field in ["id", "name", "kind"] {
                let mut forged = original.clone();
                let record = &mut forged.records[0];
                match field {
                    "id" => record.upstream_id = "forged".into(),
                    "name" => record.name = "Forged".into(),
                    "kind" => {
                        record.kind = match record.kind {
                            RecordKind::Model => RecordKind::Service,
                            RecordKind::Service => RecordKind::Model,
                        };
                    }
                    _ => unreachable!(),
                }
                let kind = match record.kind {
                    RecordKind::Model => "model",
                    RecordKind::Service => "service",
                };
                record.id = format!("{source}:{kind}:{}", record.upstream_id);
                let error = validate(forged, cache.path()).unwrap_err();
                assert!(
                    error.to_string().contains("does not match identity"),
                    "{source} {path} {field}: {error:#}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn source_name_fallbacks_remain_valid() -> Result<()> {
        let fixtures = [
            (
                "huggingface",
                "/models",
                json!({"id":"lab/a","name":" "}),
                "lab/a",
            ),
            (
                "openrouter",
                "/models",
                json!({"data":[{"id":"lab/a","name":null}]}),
                "lab/a",
            ),
            (
                "vercel",
                "/models",
                json!({"data":[{"id":"lab/a","name":false}]}),
                "lab/a",
            ),
            (
                "modelsdev",
                "/models.json",
                json!({"lab/a":{"id":"lab/a","name":""}}),
                "lab/a",
            ),
            (
                "modelsdev",
                "/catalog.json",
                json!({"providers":{"lab":{"id":"lab","models":{"a":{"id":"a","name":null}}}}}),
                "a",
            ),
            (
                "artificialanalysis",
                "/models",
                json!({"data":[{"id":"a"}]}),
                "a",
            ),
            (
                "anthropic",
                "/models",
                json!({"data":[{"id":"a","display_name":null}]}),
                "a",
            ),
            (
                "gemini",
                "/models",
                json!({"models":[{"name":"models/a"}]}),
                "models/a",
            ),
            ("deepseek", "/models", json!({"data":[{"id":"a"}]}), "a"),
            (
                "fal",
                "/models",
                json!({"models":[{"endpoint_id":"lab/a"}]}),
                "lab/a",
            ),
        ];
        for (source, path, raw, name) in fixtures {
            let cache = tempfile::tempdir()?;
            let catalog = catalog(cache.path(), source, path, raw)?;
            assert_eq!(catalog.records[0].name, name);
            validate(catalog, cache.path())?;
        }
        Ok(())
    }

    #[test]
    fn origin_must_identify_the_same_record_even_when_facts_match() -> Result<()> {
        let cache = tempfile::tempdir()?;
        let original = catalog(
            cache.path(),
            "openrouter",
            "/models",
            json!({
                "data":[{"id":"lab/a","name":"Same"},{"id":"lab/b","name":"Same"}],
                "unrelated":{}
            }),
        )?;
        validate(original.clone(), cache.path())?;
        for pointer in ["/data/1", "/unrelated"] {
            let mut forged = original.clone();
            forged.records[0].origin.pointer = pointer.into();
            assert!(validate(forged, cache.path()).is_err());
        }
        Ok(())
    }

    #[test]
    fn modelsdev_model_links_are_replayed_from_canonical_model_id() -> Result<()> {
        let cache = tempfile::tempdir()?;
        let mut original = catalog(
            cache.path(),
            "modelsdev",
            "/models.json",
            json!({
                "lab~one/a":{"id":"lab~one/a","name":"A"},
                "lab~one/b":{"id":"lab~one/b","name":"B"}
            }),
        )?;
        let services = catalog(
            cache.path(),
            "modelsdev",
            "/catalog.json",
            json!({
                "providers":{"host/one":{"id":"host/one","models":{
                    "a~b":{"id":"a~b","canonical_model_id":"lab~one/a"}
                }}}
            }),
        )?;
        original.snapshots.extend(services.snapshots);
        original.records.extend(services.records);
        validate(original.clone(), cache.path())?;
        for model in [None, Some("modelsdev:model:lab~one/b".into())] {
            let mut forged = original.clone();
            forged.records.last_mut().unwrap().model = model;
            assert!(
                validate(forged, cache.path())
                    .unwrap_err()
                    .to_string()
                    .contains("model does not match identity")
            );
        }
        Ok(())
    }

    #[test]
    fn older_fact_subsets_and_unmapped_fields_do_not_affect_identity() -> Result<()> {
        let cache = tempfile::tempdir()?;
        let mut original = catalog(
            cache.path(),
            "huggingface",
            "/models",
            json!({
                "id":"lab/a", "pipeline_tag":"text-generation", "author":"lab",
                "future_field":{"name":"a different name"}
            }),
        )?;
        assert!(original.records[0].facts.len() >= 2);
        original.records[0]
            .facts
            .retain(|key, _| key == "publisher");
        validate(original, cache.path())?;
        Ok(())
    }
}

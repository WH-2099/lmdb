use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::assertion::{self, AssertionSet};
use crate::model::{Catalog, Record, RecordKind, SCHEMA_VERSION};
use crate::store;

pub struct Registry {
    pub catalogs: Vec<Catalog>,
    pub assertions: Vec<AssertionSet>,
}

pub fn load(root: &Path) -> Result<Registry> {
    let catalogs = store::load_catalogs(root)?;
    let directory = root.join("data/assertions");
    let mut paths = Vec::new();
    match fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                let path = entry?.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "yaml")
                {
                    paths.push(path);
                }
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("read {}", directory.display())),
    }
    paths.sort();
    let assertions = paths
        .iter()
        .map(|path| store::read_yaml(path))
        .collect::<Result<_>>()?;
    Ok(Registry {
        catalogs,
        assertions,
    })
}

impl Registry {
    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.catalogs.iter().flat_map(|catalog| &catalog.records)
    }

    pub fn validate(&self, cache_root: Option<&Path>) -> Result<()> {
        let mut ids = BTreeSet::new();
        let models = self
            .records()
            .filter(|record| record.kind == RecordKind::Model)
            .map(|record| record.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut raw_snapshots = BTreeMap::<String, Value>::new();
        let mut snapshot_ids = BTreeSet::new();
        for catalog in &self.catalogs {
            ensure!(
                catalog.schema_version == SCHEMA_VERSION,
                "unsupported catalog schema version"
            );
            ensure!(
                !catalog.source.is_empty()
                    && catalog
                        .source
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "invalid source: {}",
                catalog.source
            );
            let mut snapshots = BTreeMap::new();
            for snapshot in &catalog.snapshots {
                ensure!(
                    snapshot.schema_version == SCHEMA_VERSION,
                    "unsupported snapshot schema version"
                );
                ensure!(
                    snapshot.source == catalog.source,
                    "snapshot {}: source mismatch",
                    snapshot.id
                );
                ensure!(
                    snapshot.sha256.len() == 64
                        && snapshot
                            .sha256
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                    "snapshot {}: invalid SHA-256",
                    snapshot.id
                );
                ensure!(
                    snapshot.id == format!("{}:{}", catalog.source, snapshot.sha256),
                    "invalid snapshot id: {}",
                    snapshot.id
                );
                ensure!(
                    snapshot_ids.insert(&snapshot.id),
                    "duplicate snapshot id: {}",
                    snapshot.id
                );
                let url = reqwest::Url::parse(&snapshot.url).context("invalid snapshot URL")?;
                ensure!(
                    url.scheme() == "https"
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none(),
                    "snapshot {}: URL must use HTTPS without credentials",
                    snapshot.id
                );
                time::OffsetDateTime::parse(
                    &snapshot.retrieved_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .with_context(|| format!("snapshot {}: invalid retrieved_at", snapshot.id))?;
                if let Some(cache) = cache_root
                    && !raw_snapshots.contains_key(&snapshot.sha256)
                {
                    let path = cache
                        .join("snapshots")
                        .join(format!("{}.json", snapshot.sha256));
                    let bytes =
                        fs::read(&path).with_context(|| format!("read {}", path.display()))?;
                    ensure!(
                        format!("{:x}", Sha256::digest(&bytes)) == snapshot.sha256,
                        "snapshot {}: cached SHA-256 mismatch",
                        snapshot.id
                    );
                    raw_snapshots.insert(
                        snapshot.sha256.clone(),
                        serde_json::from_slice(&bytes)
                            .with_context(|| format!("parse {}", path.display()))?,
                    );
                }
                snapshots.insert(snapshot.id.as_str(), snapshot);
            }
            for record in &catalog.records {
                let id = &record.id;
                ensure!(
                    record.schema_version == SCHEMA_VERSION,
                    "record {id}: unsupported schema version"
                );
                ensure!(
                    record.source == catalog.source,
                    "record {id}: source mismatch"
                );
                ensure!(
                    !record.upstream_id.trim().is_empty() && !record.name.trim().is_empty(),
                    "record {id}: upstream_id and name must not be blank"
                );
                let kind = match record.kind {
                    RecordKind::Model => "model",
                    RecordKind::Service => "service",
                };
                ensure!(
                    *id == format!("{}:{kind}:{}", record.source, record.upstream_id),
                    "invalid record id: {id}"
                );
                ensure!(ids.insert(id), "duplicate record id: {id}");
                if let Some(model) = &record.model {
                    ensure!(
                        record.kind == RecordKind::Service && models.contains(model.as_str()),
                        "record {id}: model must reference a model record from a service"
                    );
                }
                let snapshot = snapshots
                    .get(record.origin.snapshot.as_str())
                    .with_context(|| {
                        format!("record {id}: unknown snapshot {}", record.origin.snapshot)
                    })?;
                ensure!(
                    valid_pointer(&record.origin.pointer),
                    "record {id}: invalid origin pointer"
                );
                let raw = raw_snapshots.get(&snapshot.sha256);
                if let Some(raw) = raw {
                    ensure!(
                        raw.pointer(&record.origin.pointer)
                            .is_some_and(Value::is_object),
                        "record {id}: origin pointer must reference an object"
                    );
                }
                for (key, fact) in &record.facts {
                    ensure!(
                        !key.trim().is_empty() && !fact.value.is_null(),
                        "record {id}: fact {key} must have a name and non-null value"
                    );
                    ensure!(
                        valid_pointer(&fact.pointer),
                        "record {id}: invalid pointer for fact {key}"
                    );
                    if matches!(
                        key.as_str(),
                        "context_window" | "max_input_tokens" | "max_output_tokens"
                    ) {
                        ensure!(
                            fact.value.as_u64().is_some() && fact.unit.as_deref() == Some("tokens"),
                            "record {id}: fact {key} must be an unsigned integer in tokens"
                        );
                    }
                    if let Some(raw) = raw {
                        ensure!(
                            raw.pointer(&fact.pointer) == Some(&fact.value),
                            "record {id}: fact {key} does not match its snapshot pointer"
                        );
                    }
                }
            }
        }
        let mut assertion_ids = BTreeSet::new();
        for set in &self.assertions {
            assertion::validate(set)?;
            for item in &set.assertions {
                ensure!(
                    assertion_ids.insert(&item.id),
                    "duplicate assertion id: {}",
                    item.id
                );
            }
        }
        Ok(())
    }

    pub fn export(&self) -> Value {
        let records = self
            .records()
            .map(|record| (&record.id, record))
            .collect::<BTreeMap<_, _>>();
        let snapshots = self
            .catalogs
            .iter()
            .flat_map(|catalog| &catalog.snapshots)
            .map(|snapshot| (&snapshot.id, snapshot))
            .collect::<BTreeMap<_, _>>();
        json!({
            "schema_version": SCHEMA_VERSION,
            "models": records.values().filter(|record| record.kind == RecordKind::Model).collect::<Vec<_>>(),
            "services": records.values().filter(|record| record.kind == RecordKind::Service).collect::<Vec<_>>(),
            "snapshots": snapshots.into_values().collect::<Vec<_>>(),
            "assertion_sets": self.assertions,
        })
    }
}

fn valid_pointer(pointer: &str) -> bool {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return false;
    }
    let mut bytes = pointer.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
            return false;
        }
    }
    true
}

pub fn merge_catalog(old: Option<Catalog>, incoming: Catalog) -> Result<Catalog> {
    ensure!(
        incoming.schema_version == SCHEMA_VERSION,
        "unsupported incoming catalog version"
    );
    ensure!(
        !incoming.records.is_empty(),
        "refusing to merge an empty incoming catalog"
    );
    let mut records = BTreeMap::new();
    let mut snapshots = BTreeMap::new();
    if let Some(old) = old {
        ensure!(
            old.source == incoming.source && old.schema_version == SCHEMA_VERSION,
            "catalog source or version mismatch"
        );
        for record in old.records {
            ensure!(
                records.insert(record.id.clone(), record).is_none(),
                "duplicate old record id"
            );
        }
        for snapshot in old.snapshots {
            ensure!(
                snapshots.insert(snapshot.id.clone(), snapshot).is_none(),
                "duplicate old snapshot id"
            );
        }
    }
    let mut incoming_ids = BTreeSet::new();
    for record in incoming.records {
        ensure!(
            incoming_ids.insert(record.id.clone()),
            "duplicate incoming record id: {}",
            record.id
        );
        if records
            .get(&record.id)
            .is_none_or(|old| !same_content(old, &record))
        {
            records.insert(record.id.clone(), record);
        }
    }
    let mut incoming_snapshots = BTreeSet::new();
    for snapshot in incoming.snapshots {
        ensure!(
            incoming_snapshots.insert(snapshot.id.clone()),
            "duplicate incoming snapshot id: {}",
            snapshot.id
        );
        snapshots.entry(snapshot.id.clone()).or_insert(snapshot);
    }
    let referenced = records
        .values()
        .map(|record| record.origin.snapshot.as_str())
        .collect::<BTreeSet<_>>();
    for id in &referenced {
        ensure!(snapshots.contains_key(*id), "unknown merged snapshot: {id}");
    }
    snapshots.retain(|id, _| referenced.contains(id.as_str()));
    Ok(Catalog {
        schema_version: SCHEMA_VERSION,
        source: incoming.source,
        snapshots: snapshots.into_values().collect(),
        records: records.into_values().collect(),
    })
}

fn same_content(old: &Record, new: &Record) -> bool {
    old.name == new.name
        && old.kind == new.kind
        && old.source == new.source
        && old.upstream_id == new.upstream_id
        && old.model == new.model
        && old.facts.len() == new.facts.len()
        && old.facts.iter().all(|(key, fact)| {
            new.facts
                .get(key)
                .is_some_and(|new| fact.value == new.value && fact.unit == new.unit)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Fact, Origin, Snapshot};

    fn catalog(raw: &[u8]) -> Catalog {
        let sha256 = format!("{:x}", Sha256::digest(raw));
        let snapshot = Snapshot {
            schema_version: 1,
            id: format!("test:{sha256}"),
            source: "test".into(),
            url: "https://example.com/models".into(),
            retrieved_at: "2026-10-15T00:00:00Z".into(),
            sha256,
            upstream_revision: None,
        };
        Catalog {
            schema_version: 1,
            source: "test".into(),
            records: vec![Record {
                schema_version: 1,
                kind: RecordKind::Model,
                id: "test:model:a".into(),
                source: "test".into(),
                upstream_id: "a".into(),
                name: "A".into(),
                model: None,
                origin: Origin {
                    snapshot: snapshot.id.clone(),
                    pointer: "/data/0".into(),
                },
                facts: BTreeMap::from([(
                    "context_window".into(),
                    Fact {
                        value: json!(42),
                        pointer: "/data/0/context".into(),
                        unit: Some("tokens".into()),
                    },
                )]),
            }],
            snapshots: vec![snapshot],
        }
    }

    fn registry(catalog: Catalog) -> Registry {
        Registry {
            catalogs: vec![catalog],
            assertions: vec![],
        }
    }

    #[test]
    fn merge_preserves_unchanged_evidence_and_missing_records() -> Result<()> {
        let mut old = catalog(br#"{"data":[{"context":42}]}"#);
        let mut missing = old.records[0].clone();
        missing.id = "test:model:missing".into();
        missing.upstream_id = "missing".into();
        old.records.push(missing);
        let mut incoming = catalog(br#"{"data":[{}, {"context":42}]}"#);
        incoming.records[0].origin.pointer = "/data/1".into();
        incoming.records[0]
            .facts
            .get_mut("context_window")
            .unwrap()
            .pointer = "/data/1/context".into();
        assert_eq!(merge_catalog(Some(old.clone()), incoming)?, old);
        let mut changed = catalog(br#"{"data":[{"context":50}]}"#);
        changed.records[0]
            .facts
            .get_mut("context_window")
            .unwrap()
            .value = json!(50);
        let merged = merge_catalog(Some(old.clone()), changed.clone())?;
        assert_eq!(merged.records[0], changed.records[0]);
        assert_eq!(merged.records[1], old.records[1]);
        assert_eq!(merged.snapshots.len(), 2);
        let mut repeated = old.clone();
        repeated.snapshots[0].retrieved_at = "2026-10-16T00:00:00Z".into();
        assert_eq!(merge_catalog(Some(old.clone()), repeated)?, old);
        Ok(())
    }

    #[test]
    fn rejects_duplicates_empty_updates_and_dangling_links() {
        let original = catalog(b"{}");
        let mut duplicate = original.clone();
        duplicate.records.push(duplicate.records[0].clone());
        assert!(merge_catalog(None, duplicate.clone()).is_err());
        assert!(registry(duplicate).validate(None).is_err());
        let mut empty = original.clone();
        empty.records.clear();
        assert!(merge_catalog(Some(original.clone()), empty).is_err());
        let mut dangling = original.clone();
        dangling.records[0].origin.snapshot = "missing".into();
        assert!(registry(dangling).validate(None).is_err());
        let mut dangling = original;
        dangling.records[0].kind = RecordKind::Service;
        dangling.records[0].id = "test:service:a".into();
        dangling.records[0].model = Some("test:model:missing".into());
        assert!(registry(dangling).validate(None).is_err());
    }

    #[test]
    fn verifies_hash_and_replays_fact_and_origin_pointers() -> Result<()> {
        let raw = br#"{"data":[{"context":42}]}"#;
        let original = catalog(raw);
        let temporary = tempfile::tempdir()?;
        let path = temporary
            .path()
            .join("snapshots")
            .join(format!("{}.json", original.snapshots[0].sha256));
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, raw)?;
        registry(original.clone()).validate(Some(temporary.path()))?;
        let mut mismatch = original.clone();
        mismatch.records[0]
            .facts
            .get_mut("context_window")
            .unwrap()
            .value = json!(43);
        assert!(
            registry(mismatch)
                .validate(Some(temporary.path()))
                .unwrap_err()
                .to_string()
                .contains("does not match")
        );
        let mut mismatch = original.clone();
        mismatch.records[0].origin.pointer = "/data/0/context".into();
        assert!(
            registry(mismatch)
                .validate(Some(temporary.path()))
                .unwrap_err()
                .to_string()
                .contains("reference an object")
        );
        fs::write(path, b"{}")?;
        assert!(
            registry(original)
                .validate(Some(temporary.path()))
                .unwrap_err()
                .to_string()
                .contains("SHA-256 mismatch")
        );
        Ok(())
    }

    #[test]
    fn checks_pointer_syntax_and_token_units() {
        for pointer in ["", "/", "/a~0b/~1/名字", "/~01"] {
            assert!(valid_pointer(pointer), "{pointer}");
        }
        for pointer in ["a", "/~", "/~2", "/~00~"] {
            assert!(!valid_pointer(pointer), "{pointer}");
        }
        for value in [json!(-1), json!(1.5), json!("42"), Value::Null] {
            let mut invalid = catalog(b"{}");
            invalid.records[0]
                .facts
                .get_mut("context_window")
                .unwrap()
                .value = value;
            assert!(registry(invalid).validate(None).is_err());
        }
        let mut invalid = catalog(b"{}");
        invalid.records[0]
            .facts
            .get_mut("context_window")
            .unwrap()
            .unit = None;
        assert!(registry(invalid).validate(None).is_err());
    }
}

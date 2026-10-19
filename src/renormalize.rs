use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{adapters, model::Record, registry, store};

#[derive(Debug, Serialize)]
pub struct Summary {
    pub source: String,
    pub records: usize,
    pub changed: usize,
}

/// Rebuild imported fields from their original snapshots without advancing observation times.
pub fn run(root: &Path, sources: &[String]) -> Result<Vec<Summary>> {
    ensure!(
        !sources.is_empty(),
        "renormalize requires explicit --source"
    );
    let mut registry = registry::load(root)?;
    registry.validate(None)?;
    let mut selected = Vec::new();
    for source in sources {
        let index = registry
            .catalogs
            .iter()
            .position(|catalog| catalog.source == *source)
            .with_context(|| format!("no saved catalog for source {source}"))?;
        ensure!(!selected.contains(&index), "duplicate source: {source}");
        selected.push(index);
    }
    let cache = root.join(".cache");
    let selected_registry = registry::Registry {
        catalogs: selected
            .iter()
            .map(|&index| registry.catalogs[index].clone())
            .collect(),
        assertions: vec![],
    };
    selected_registry.validate(Some(&cache))?;
    let mut summaries = Vec::new();
    for &index in &selected {
        let catalog = &mut registry.catalogs[index];
        let mut replayed = BTreeMap::<String, BTreeMap<String, Record>>::new();
        let mut changed = 0;
        for record in &mut catalog.records {
            if !replayed.contains_key(&record.origin.snapshot) {
                let snapshot = catalog
                    .snapshots
                    .iter()
                    .find(|snapshot| snapshot.id == record.origin.snapshot)
                    .context("record snapshot missing")?;
                let bytes = fs::read(
                    cache
                        .join("snapshots")
                        .join(format!("{}.json", snapshot.sha256)),
                )?;
                let value = serde_json::from_slice(&bytes)?;
                let records = adapters::normalize(&value, snapshot)?
                    .with_context(|| format!("source {} has no normalizer", catalog.source))?;
                let mut by_origin = BTreeMap::new();
                for normalized in records {
                    ensure!(
                        by_origin
                            .insert(normalized.origin.pointer.clone(), normalized)
                            .is_none(),
                        "normalizer returned duplicate origins for {}",
                        snapshot.id
                    );
                }
                replayed.insert(snapshot.id.clone(), by_origin);
            }
            let replacement = replayed[&record.origin.snapshot]
                .get(&record.origin.pointer)
                .with_context(|| format!("{}: original record is absent from replay", record.id))?;
            ensure!(
                record.id == replacement.id
                    && record.upstream_id == replacement.upstream_id
                    && record.name == replacement.name
                    && record.kind == replacement.kind
                    && record.model == replacement.model
                    && record.origin == replacement.origin,
                "{}: replay changed identity; review the evidence before migrating",
                record.id
            );
            if record != replacement {
                *record = replacement.clone();
                changed += 1;
            }
        }
        summaries.push(Summary {
            source: catalog.source.clone(),
            records: catalog.records.len(),
            changed,
        });
    }
    registry.validate(None)?;
    for index in selected {
        let catalog = &registry.catalogs[index];
        store::write_yaml(
            &root
                .join("data/catalogs")
                .join(format!("{}.yaml", catalog.source)),
            catalog,
        )?;
    }
    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Catalog, Snapshot, SyncManifest};
    use sha2::{Digest, Sha256};

    #[test]
    fn replay_adds_cached_fields_preserving_identity_snapshot_and_observation() -> Result<()> {
        let root = tempfile::tempdir()?;
        let raw = br#"{"id":"owner/model","author":"owner","sha":"abc","cardData":{"license":"other","license_name":"Custom Terms","datasets":["custom"]}}"#;
        let hash = format!("{:x}", Sha256::digest(raw));
        let snapshot = Snapshot {
            schema_version: 1,
            id: format!("huggingface:{hash}"),
            source: "huggingface".into(),
            url: "https://huggingface.co/api/models/owner/model".into(),
            retrieved_at: "2026-10-15T00:00:00Z".into(),
            sha256: hash.clone(),
            upstream_revision: None,
        };
        let mut record = adapters::normalize(&serde_json::from_slice(raw)?, &snapshot)?
            .unwrap()
            .remove(0);
        record.facts.remove("license_name");
        record.facts.remove("datasets");
        let catalog = Catalog {
            schema_version: 1,
            source: "huggingface".into(),
            last_sync: Some(SyncManifest {
                completed_at: "2026-10-15T01:00:00Z".into(),
                returned_ids: vec![record.id.clone()],
                snapshot_ids: vec![snapshot.id.clone()],
                ..Default::default()
            }),
            snapshots: vec![snapshot],
            records: vec![record],
        };
        store::write_bytes(
            &root
                .path()
                .join(".cache/snapshots")
                .join(format!("{hash}.json")),
            raw,
        )?;
        store::write_yaml(
            &root.path().join("data/catalogs/huggingface.yaml"),
            &catalog,
        )?;
        let result = run(root.path(), &["huggingface".into()])?;
        assert_eq!(result[0].changed, 1);
        let updated = registry::load(root.path())?.catalogs.remove(0);
        assert_eq!(updated.last_sync, catalog.last_sync);
        assert_eq!(updated.snapshots, catalog.snapshots);
        assert_eq!(updated.records[0].origin, catalog.records[0].origin);
        assert_eq!(
            updated.records[0].facts["license_name"].value,
            "Custom Terms"
        );
        assert_eq!(run(root.path(), &["huggingface".into()])?[0].changed, 0);
        let saved = fs::read(root.path().join("data/catalogs/huggingface.yaml"))?;
        assert!(run(root.path(), &["huggingface".into(), "missing".into()]).is_err());
        assert_eq!(
            fs::read(root.path().join("data/catalogs/huggingface.yaml"))?,
            saved
        );
        Ok(())
    }
}

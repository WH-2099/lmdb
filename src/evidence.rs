use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{documents::DocumentCatalog, registry::Registry, store};

const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DATA_FILE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    data_tree_sha256: String,
    data_files: BTreeMap<String, String>,
    snapshots: BTreeSet<String>,
    documents: BTreeSet<String>,
}

/// Write only after both catalog and document bodies have been archived successfully.
pub fn write_manifest(
    root: &Path,
    registry: &Registry,
    documents: &DocumentCatalog,
    archive_dir: &Path,
) -> Result<bool> {
    let manifest = manifest(root, registry, documents)?;
    let mut bytes = serde_json::to_vec_pretty(&manifest)?;
    bytes.push(b'\n');
    ensure!(
        bytes.len() as u64 <= MAX_MANIFEST_BYTES,
        "evidence manifest exceeds the 4 MiB limit"
    );
    store::write_bytes(&archive_dir.join("manifest.json"), &bytes)
}

/// Check metadata and all referenced hashes before restoring any archive bodies.
pub fn validate_manifest(
    root: &Path,
    registry: &Registry,
    documents: &DocumentCatalog,
    archive_dir: &Path,
) -> Result<()> {
    let path = archive_dir.join("manifest.json");
    let file =
        File::open(&path).with_context(|| format!("open evidence manifest {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_MANIFEST_BYTES,
        "evidence manifest exceeds the 4 MiB limit"
    );
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_MANIFEST_BYTES,
        "evidence manifest exceeds the 4 MiB limit"
    );
    let archived: Manifest = serde_json::from_slice(&bytes).context("parse evidence manifest")?;
    let expected = manifest(root, registry, documents)?;
    ensure!(
        archived.schema_version == 1,
        "unsupported evidence manifest version"
    );
    ensure!(
        archived.data_tree_sha256 == expected.data_tree_sha256
            && archived.data_files == expected.data_files,
        "evidence archive belongs to a different data tree; create an archive for the current data"
    );
    ensure!(
        archived.snapshots == expected.snapshots && archived.documents == expected.documents,
        "evidence manifest snapshot hashes differ from the current catalogs"
    );
    Ok(())
}

fn manifest(root: &Path, registry: &Registry, documents: &DocumentCatalog) -> Result<Manifest> {
    let mut data_files = BTreeMap::new();
    collect_data_files(root, &root.join("data"), &mut data_files)?;
    // Sorted relative paths and their byte hashes bind the archive to an exact data tree.
    let data_tree_sha256 = format!("{:x}", Sha256::digest(serde_json::to_vec(&data_files)?));
    Ok(Manifest {
        schema_version: 1,
        data_tree_sha256,
        data_files,
        snapshots: registry
            .catalogs
            .iter()
            .flat_map(|catalog| &catalog.snapshots)
            .map(|snapshot| snapshot.sha256.clone())
            .collect(),
        documents: documents
            .documents
            .iter()
            .flat_map(|document| {
                document
                    .content
                    .as_ref()
                    .map(|content| &content.snapshot)
                    .into_iter()
                    .chain(document.failed_snapshot.as_ref())
            })
            .map(|snapshot| snapshot.sha256.clone())
            .collect(),
    })
}

fn collect_data_files(
    root: &Path,
    directory: &Path,
    hashes: &mut BTreeMap<String, String>,
) -> Result<()> {
    ensure!(
        fs::symlink_metadata(directory)?.file_type().is_dir(),
        "{} must be a directory without symbolic links",
        directory.display()
    );
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_data_files(root, &path, hashes)?;
            continue;
        }
        ensure!(
            file_type.is_file(),
            "{} must be a regular data file",
            path.display()
        );
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .context("data filenames must be UTF-8")?
            .replace(std::path::MAIN_SEPARATOR, "/");
        let file = File::open(&path)?;
        ensure!(
            file.metadata()?.len() <= MAX_DATA_FILE_BYTES,
            "{} exceeds the 128 MiB data file limit",
            path.display()
        );
        let mut hasher = Sha256::new();
        let bytes = std::io::copy(&mut file.take(MAX_DATA_FILE_BYTES + 1), &mut hasher)?;
        ensure!(
            bytes <= MAX_DATA_FILE_BYTES,
            "{} exceeds the 128 MiB data file limit",
            path.display()
        );
        hashes.insert(relative, format!("{:x}", hasher.finalize()));
    }
    Ok(())
}

pub fn archive(registry: &Registry, cache_root: &Path, archive_dir: &Path) -> Result<usize> {
    copy_snapshots(registry, cache_root, archive_dir)
}

pub fn restore(registry: &Registry, archive_dir: &Path, cache_root: &Path) -> Result<usize> {
    copy_snapshots(registry, archive_dir, cache_root)
}

fn copy_snapshots(registry: &Registry, source: &Path, destination: &Path) -> Result<usize> {
    registry.validate(None)?;
    let hashes = registry
        .catalogs
        .iter()
        .flat_map(|catalog| &catalog.snapshots)
        .map(|snapshot| snapshot.sha256.as_str())
        .collect::<BTreeSet<_>>();
    let mut written = 0;
    for hash in hashes {
        let filename = format!("{hash}.json");
        let path = source.join("snapshots").join(&filename);
        let file =
            File::open(&path).with_context(|| format!("open snapshot {}", path.display()))?;
        ensure!(
            file.metadata()?.len() <= MAX_SNAPSHOT_BYTES,
            "{} exceeds the 64 MiB snapshot limit",
            path.display()
        );
        let mut bytes = Vec::new();
        file.take(MAX_SNAPSHOT_BYTES + 1)
            .read_to_end(&mut bytes)
            .with_context(|| format!("read snapshot {}", path.display()))?;
        ensure!(
            bytes.len() as u64 <= MAX_SNAPSHOT_BYTES,
            "{} exceeds the 64 MiB snapshot limit",
            path.display()
        );
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == hash,
            "{}: snapshot SHA-256 mismatch",
            path.display()
        );
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .with_context(|| format!("{}: invalid JSON snapshot", path.display()))?;
        written += usize::from(store::write_bytes(
            &destination.join("snapshots").join(filename),
            &bytes,
        )?);
    }
    registry.validate(Some(destination))?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use serde_json::json;

    use super::*;
    use crate::model::{Catalog, Snapshot, SyncManifest};

    const RAW: &[u8] = b"{\n  \"model\": { \"context\": 42 }\n}\n";

    fn snapshot(raw: &[u8]) -> Snapshot {
        let sha256 = format!("{:x}", Sha256::digest(raw));
        Snapshot {
            schema_version: 1,
            id: format!("test:{sha256}"),
            source: "test".into(),
            url: "https://example.com/models".into(),
            retrieved_at: "2026-10-16T00:00:00Z".into(),
            sha256,
            upstream_revision: None,
        }
    }

    fn registry(raw: &[u8]) -> Registry {
        let snapshot = snapshot(raw);
        let catalog: Catalog = serde_json::from_value(json!({
            "schema_version": 1,
            "source": "test",
            "snapshots": [snapshot],
            "records": [{
                "schema_version": 1,
                "kind": "model",
                "id": "test:model:a",
                "source": "test",
                "upstream_id": "a",
                "name": "A",
                "origin": {"snapshot": snapshot.id, "pointer": "/model"},
                "facts": {
                    "context_window": {"value": 42, "pointer": "/model/context", "unit": "tokens"}
                }
            }]
        }))
        .unwrap();
        Registry {
            catalogs: vec![catalog],
            assertions: vec![],
        }
    }

    fn path(root: &Path, raw: &[u8]) -> PathBuf {
        root.join("snapshots")
            .join(format!("{}.json", snapshot(raw).sha256))
    }

    #[test]
    fn manifest_binds_data_bytes_and_all_snapshot_hashes() -> Result<()> {
        let root = tempfile::tempdir()?;
        let archive_dir = tempfile::tempdir()?;
        let registry = registry(RAW);
        let failed = snapshot(b"failed page body");
        let documents: DocumentCatalog = serde_json::from_value(json!({
            "schema_version": 1,
            "documents": [{
                "source": {
                    "id": "test-page", "vendor": "test", "kind": "model_card",
                    "url": "https://example.com/model", "format": "markdown", "records": []
                },
                "checked_at": "2026-10-16T00:00:00Z",
                "error": "page extraction failed",
                "failed_snapshot": failed
            }]
        }))?;
        let data = root.path().join("data/catalogs/test.yaml");
        store::write_bytes(&data, b"original data\n")?;
        assert!(write_manifest(
            root.path(),
            &registry,
            &documents,
            archive_dir.path()
        )?);
        assert!(!write_manifest(
            root.path(),
            &registry,
            &documents,
            archive_dir.path()
        )?);
        validate_manifest(root.path(), &registry, &documents, archive_dir.path())?;
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(archive_dir.path().join("manifest.json"))?)?;
        assert_eq!(saved["documents"], json!([failed.sha256]));

        fs::write(&data, b"changed data\n")?;
        let error =
            validate_manifest(root.path(), &registry, &documents, archive_dir.path()).unwrap_err();
        assert!(error.to_string().contains("different data tree"));
        fs::write(&data, b"original data\n")?;

        let mut changed_registry = Registry {
            catalogs: registry.catalogs.clone(),
            assertions: registry.assertions.clone(),
        };
        changed_registry.catalogs[0].snapshots[0].sha256 = "0".repeat(64);
        let error = validate_manifest(
            root.path(),
            &changed_registry,
            &documents,
            archive_dir.path(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("snapshot hashes differ"));

        let path = archive_dir.path().join("manifest.json");
        let mut edited: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        edited["documents"] = json!(["0".repeat(64)]);
        fs::write(&path, serde_json::to_vec(&edited)?)?;
        let error =
            validate_manifest(root.path(), &registry, &documents, archive_dir.path()).unwrap_err();
        assert!(error.to_string().contains("snapshot hashes differ"));
        Ok(())
    }

    #[test]
    fn manifest_rejects_missing_and_oversized_input() -> Result<()> {
        let root = tempfile::tempdir()?;
        let archive_dir = tempfile::tempdir()?;
        let registry = registry(RAW);
        let documents = DocumentCatalog {
            schema_version: 1,
            documents: vec![],
        };
        let error =
            validate_manifest(root.path(), &registry, &documents, archive_dir.path()).unwrap_err();
        assert!(error.to_string().contains("open evidence manifest"));
        File::create(archive_dir.path().join("manifest.json"))?.set_len(MAX_MANIFEST_BYTES + 1)?;
        let error =
            validate_manifest(root.path(), &registry, &documents, archive_dir.path()).unwrap_err();
        assert!(error.to_string().contains("4 MiB"));
        Ok(())
    }

    #[test]
    fn roundtrip_preserves_bytes_and_all_snapshots_without_http_receipts() -> Result<()> {
        let source = tempfile::tempdir()?;
        let archive_dir = tempfile::tempdir()?;
        let fresh_cache = tempfile::tempdir()?;
        let mut registry = registry(RAW);
        let mut duplicate = registry.catalogs[0].clone();
        duplicate.source = "other".into();
        duplicate.records.clear();
        duplicate.snapshots[0].source = "other".into();
        duplicate.snapshots[0].id = format!("other:{}", duplicate.snapshots[0].sha256);
        registry.catalogs.push(duplicate);
        let extra = b"{\"observed\": true}\n";
        let extra_snapshot = snapshot(extra);
        registry.catalogs[0].last_sync = Some(SyncManifest {
            completed_at: "2026-10-16T00:01:00Z".into(),
            returned_ids: vec!["test:model:a".into()],
            missing_ids: vec![],
            snapshot_ids: vec![extra_snapshot.id.clone()],
            ..Default::default()
        });
        registry.catalogs[0].snapshots.push(extra_snapshot);
        store::write_bytes(&path(source.path(), RAW), RAW)?;
        store::write_bytes(&path(source.path(), extra), extra)?;
        store::write_bytes(&source.path().join("http/receipt.json"), b"{}")?;

        assert_eq!(archive(&registry, source.path(), archive_dir.path())?, 2);
        assert_eq!(archive(&registry, source.path(), archive_dir.path())?, 0);
        assert!(!archive_dir.path().join("http").exists());
        assert_eq!(
            restore(&registry, archive_dir.path(), fresh_cache.path())?,
            2
        );
        assert_eq!(
            restore(&registry, archive_dir.path(), fresh_cache.path())?,
            0
        );
        assert!(!fresh_cache.path().join("http").exists());
        assert_eq!(fs::read(path(fresh_cache.path(), RAW))?, RAW);
        assert_eq!(fs::read(path(fresh_cache.path(), extra))?, extra);

        fs::write(path(fresh_cache.path(), RAW), b"damaged")?;
        assert_eq!(
            restore(&registry, archive_dir.path(), fresh_cache.path())?,
            1
        );
        assert_eq!(fs::read(path(fresh_cache.path(), RAW))?, RAW);
        Ok(())
    }

    #[test]
    fn corrupt_or_missing_archive_does_not_replace_existing_cache() -> Result<()> {
        let archive_dir = tempfile::tempdir()?;
        let cache = tempfile::tempdir()?;
        let registry = registry(RAW);
        let archived = path(archive_dir.path(), RAW);
        let cached = path(cache.path(), RAW);
        store::write_bytes(&archived, b"{}")?;
        store::write_bytes(&cached, RAW)?;

        let error = restore(&registry, archive_dir.path(), cache.path()).unwrap_err();
        assert!(error.to_string().contains("SHA-256 mismatch"));
        assert_eq!(fs::read(&cached)?, RAW);
        fs::remove_file(&archived)?;
        let error = restore(&registry, archive_dir.path(), cache.path()).unwrap_err();
        assert!(error.to_string().contains(&snapshot(RAW).sha256));
        assert_eq!(fs::read(&cached)?, RAW);
        Ok(())
    }

    #[test]
    fn invalid_json_and_oversized_snapshots_are_rejected_before_writing() -> Result<()> {
        let source = tempfile::tempdir()?;
        let destination = tempfile::tempdir()?;
        let invalid = b"not JSON";
        store::write_bytes(&path(source.path(), invalid), invalid)?;
        let error = archive(&registry(invalid), source.path(), destination.path()).unwrap_err();
        assert!(error.to_string().contains("invalid JSON"));
        assert!(!destination.path().join("snapshots").exists());

        File::create(path(source.path(), RAW))?.set_len(MAX_SNAPSHOT_BYTES + 1)?;
        let error = archive(&registry(RAW), source.path(), destination.path()).unwrap_err();
        assert!(error.to_string().contains("64 MiB"));
        assert!(!destination.path().join("snapshots").exists());
        Ok(())
    }

    #[test]
    fn restoration_checks_catalog_facts_against_raw_evidence() -> Result<()> {
        let source = tempfile::tempdir()?;
        let destination = tempfile::tempdir()?;
        store::write_bytes(&path(source.path(), RAW), RAW)?;
        let mut registry = registry(RAW);
        registry.catalogs[0].records[0]
            .facts
            .get_mut("context_window")
            .unwrap()
            .value = json!(43);

        let error = restore(&registry, source.path(), destination.path()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not match its snapshot pointer")
        );
        Ok(())
    }
}

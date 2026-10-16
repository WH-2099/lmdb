use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use anyhow::Result;
use model_metadata::{
    adapters::modelsdev,
    model::{Catalog, RecordKind, Snapshot},
    store,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lmdb"))
        .env_clear()
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run lmdb")
}

fn success(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn seed(root: &Path) -> Result<Catalog> {
    let mut catalog = Catalog {
        schema_version: 1,
        source: "modelsdev".into(),
        last_sync: None,
        snapshots: vec![],
        records: vec![],
    };
    for (name, raw) in [
        (
            "models",
            include_bytes!("fixtures/modelsdev-models.json").as_slice(),
        ),
        (
            "catalog",
            include_bytes!("fixtures/modelsdev-catalog.json").as_slice(),
        ),
    ] {
        let sha256 = format!("{:x}", Sha256::digest(raw));
        let snapshot = Snapshot {
            schema_version: 1,
            id: format!("modelsdev:{sha256}"),
            source: "modelsdev".into(),
            url: format!("https://models.dev/{name}.json?type=all"),
            retrieved_at: "2026-10-15T00:00:00Z".into(),
            sha256,
            upstream_revision: None,
        };
        let value = serde_json::from_slice(raw)?;
        catalog.records.extend(if name == "models" {
            modelsdev::models(&value, &snapshot)?
        } else {
            modelsdev::services(&value, &snapshot)?
        });
        store::write_bytes(
            &root
                .join(".cache/snapshots")
                .join(format!("{}.json", snapshot.sha256)),
            raw,
        )?;
        catalog.snapshots.push(snapshot);
    }
    store::write_yaml(&root.join("data/catalogs/modelsdev.yaml"), &catalog)?;
    Ok(catalog)
}

#[test]
fn generated_schemas_roundtrip_and_detect_drift() -> Result<()> {
    let root = tempfile::tempdir()?;
    success(root.path(), &["schema"]);
    success(root.path(), &["schema", "--check"]);
    for name in ["catalog", "assertions"] {
        let path = root.path().join(format!("schemas/{name}.schema.json"));
        let original = fs::read(&path)?;
        let schema: Value = serde_json::from_slice(&original)?;
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        fs::write(&path, b"{}\n")?;
        let output = run(root.path(), &["schema", "--check"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("schema drift"));
        fs::write(path, original)?;
    }
    Ok(())
}

#[test]
fn catalogs_roundtrip_and_invalid_evidence_or_references_fail() -> Result<()> {
    let root = tempfile::tempdir()?;
    let catalog = seed(root.path())?;
    success(root.path(), &["validate"]);
    success(root.path(), &["validate", "--evidence"]);
    success(root.path(), &["export"]);
    let path = root.path().join("dist/registry.json");
    let bytes = fs::read(&path)?;
    success(root.path(), &["export"]);
    assert_eq!(bytes, fs::read(&path)?);
    let export: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(export["schema_version"], 1);
    assert_eq!(export["snapshots"].as_array().unwrap().len(), 2);
    for (kind, field) in [("model", "models"), ("service", "services")] {
        let records: Value = serde_json::from_slice(&success(
            root.path(),
            &["query", "--source", "modelsdev", "--kind", kind],
        ))?;
        assert_eq!(records, export[field]);
        assert_eq!(records.as_array().unwrap().len(), 2);
    }
    let vision: Value = serde_json::from_slice(&success(
        root.path(),
        &["query", "VISION", "--kind", "model", "--limit", "1"],
    ))?;
    assert_eq!(vision[0]["id"], "modelsdev:model:lab~one/vision/one");
    assert_eq!(vision[0]["facts"]["max_output_tokens"]["value"], 0);
    assert_eq!(vision[0]["facts"]["reasoning"]["value"], false);

    let raw_path = root
        .path()
        .join(".cache/snapshots")
        .join(format!("{}.json", catalog.snapshots[0].sha256));
    fs::write(raw_path, b"{}")?;
    success(root.path(), &["validate"]);
    let output = run(root.path(), &["validate", "--evidence"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("SHA-256 mismatch"));

    for dangling_model in [false, true] {
        let mut invalid = catalog.clone();
        if dangling_model {
            invalid
                .records
                .iter_mut()
                .find(|r| r.kind == RecordKind::Service)
                .unwrap()
                .model = Some("modelsdev:model:missing".into());
        } else {
            invalid.records[0].id = "invalid-id".into();
        }
        store::write_yaml(&root.path().join("data/catalogs/modelsdev.yaml"), &invalid)?;
        for args in [&["validate"][..], &["export"][..]] {
            assert!(!run(root.path(), args).status.success());
        }
        assert_eq!(bytes, fs::read(&path)?);
    }
    Ok(())
}

#[test]
fn authenticated_sources_are_rejected_before_reading_data_or_creating_cache() -> Result<()> {
    let root = tempfile::tempdir()?;
    let catalog = root.path().join("data/catalogs/invalid.yaml");
    fs::create_dir_all(catalog.parent().unwrap())?;
    fs::write(catalog, "schema_version: [")?;
    for source in ["openai", "openrouter,openai"] {
        let output = run(root.path(), &["sync", "--source", source]);
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("openai requires explicit --authenticated"),
            "{error}"
        );
        assert!(!error.contains("fetching metadata"), "{error}");
        assert!(!root.path().join(".cache").exists());
    }
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let root = tempfile::tempdir()?;
        let output = Command::new(env!("CARGO_BIN_EXE_lmdb"))
            .env_clear()
            .env(
                "ANTHROPIC_API_KEY",
                OsString::from_vec(b"LOCAL_SENTINEL_SECRET_\xff_END".to_vec()),
            )
            .arg("--root")
            .arg(root.path())
            .args(["sync", "--source", "anthropic", "--authenticated"])
            .output()?;
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("ANTHROPIC_API_KEY"), "{error}");
        assert!(!error.contains("SENTINEL"));
        assert!(!root.path().join(".cache").exists());
    }
    Ok(())
}

use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};

use anyhow::Result;
use model_metadata::{
    assertion::AssertionSet,
    identity::IdentitySet,
    model::{Catalog, Fact, Record, Snapshot, SyncManifest, pointer_segment},
    store,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MODEL: &str = "huggingface:model:lab/alpha";
const MIRROR: &str = "modelsdev:model:lab/alpha";
const NATIVE: &str = "modelsdev:service:native/alpha";
const HOSTED: &str = "openrouter:service:host/alpha";
const UNKNOWN: &str = "huggingface:model:lab/unknown";
const ZERO: &str = "huggingface:model:lab/zero";

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

fn output(root: &Path, args: &[&str]) -> Value {
    serde_json::from_slice(&success(root, args)).expect("JSON command output")
}

fn ids(records: &Value) -> Vec<&str> {
    records
        .as_array()
        .expect("raw record array")
        .iter()
        .map(|record| record["id"].as_str().unwrap())
        .collect()
}

fn seed(root: &Path) -> Result<Vec<Catalog>> {
    let mut catalogs = Vec::new();
    for (source, raw) in [
        (
            "huggingface",
            json!([
                {"kind":"model", "id":"lab/alpha", "facts":{
                    "task":"text-ranking", "input_modalities":["text","image"],
                    "output_modalities":["text"], "context_window":4096,
                    "reasoning":true, "tool_call":true, "structured_output":false,
                    "open_weights":true, "base_models":["lab/base"]
                }},
                {"kind":"model", "id":"lab/zero", "facts":{
                    "task":"feature-extraction", "context_window":0,
                    "reasoning":false, "tool_call":false,
                    "structured_output":true, "open_weights":false
                }},
                {"kind":"model", "id":"lab/unknown", "facts":{"task":"future-task"}}
            ]),
        ),
        (
            "modelsdev",
            json!([
                {"kind":"model", "id":"lab/alpha", "facts":{
                    "weights":[{"url":"https://huggingface.co/lab/alpha"}],
                    "open_weights":true
                }},
                {"kind":"service", "id":"native/alpha", "model":MIRROR,
                 "facts":{"canonical_model_id":"lab/alpha", "tool_call":true}}
            ]),
        ),
        (
            "openrouter",
            json!([
                {"kind":"service", "id":"host/alpha", "facts":{
                    "hugging_face_id":"lab/alpha", "input_modalities":["text"],
                    "output_modalities":["speech"]
                }}
            ]),
        ),
    ] {
        let mut bytes = serde_json::to_vec_pretty(&raw)?;
        bytes.push(b'\n');
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let snapshot = Snapshot {
            schema_version: 1,
            id: format!("{source}:{sha256}"),
            source: source.into(),
            url: format!("https://example.com/{source}.json"),
            retrieved_at: "2026-10-16T00:00:00Z".into(),
            sha256,
            upstream_revision: None,
        };
        let records = raw
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let upstream_id = item["id"].as_str().unwrap();
                let kind = item["kind"].as_str().unwrap();
                let facts = item["facts"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            Fact {
                                value: value.clone(),
                                pointer: format!("/{index}/facts/{}", pointer_segment(key)),
                                unit: (key == "context_window").then(|| "tokens".into()),
                            },
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                serde_json::from_value::<Record>(json!({
                    "schema_version":1, "kind":kind,
                    "id":format!("{source}:{kind}:{upstream_id}"),
                    "source":source, "upstream_id":upstream_id, "name":upstream_id,
                    "model":item.get("model"),
                    "origin":{"snapshot":snapshot.id, "pointer":format!("/{index}")},
                    "facts":facts
                }))
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut returned_ids: Vec<_> = records
            .iter()
            .filter(|record| record.id != UNKNOWN)
            .map(|record| record.id.clone())
            .collect();
        returned_ids.sort();
        let catalog = Catalog {
            schema_version: 1,
            source: source.into(),
            last_sync: Some(SyncManifest {
                completed_at: "2026-10-16T00:01:00Z".into(),
                returned_ids,
                missing_ids: records
                    .iter()
                    .filter(|record| record.id == UNKNOWN)
                    .map(|record| record.id.clone())
                    .collect(),
                snapshot_ids: vec![snapshot.id.clone()],
            }),
            snapshots: vec![snapshot],
            records,
        };
        store::write_bytes(
            &root
                .join(".cache/snapshots")
                .join(format!("{}.json", catalog.snapshots[0].sha256)),
            &bytes,
        )?;
        store::write_yaml(&root.join(format!("data/catalogs/{source}.yaml")), &catalog)?;
        catalogs.push(catalog);
    }
    let sources = json!({
        "model-doc":{"url":"https://example.com/release", "checked_on":"2026-10-16"},
        "interface-doc":{"url":"https://example.com/api", "checked_on":"2026-10-16"}
    });
    let proof = |source| {
        json!([{
            "source":source, "locator":"Explicit release and API documentation",
            "checked_on":"2026-10-16", "kind":"reported"
        }])
    };
    let identities: IdentitySet = serde_json::from_value(json!({
        "schema_version":1, "sources":sources,
        "models":[{"id":"lab/alpha", "name":"Alpha", "records":[MODEL,MIRROR],
            "evidence":proof("model-doc")}],
        "interfaces":[{"id":"lab/api", "name":"Native API", "records":[NATIVE],
            "evidence":proof("interface-doc")}]
    }))?;
    store::write_yaml(&root.join("data/identities/core.yaml"), &identities)?;
    let assertions: AssertionSet = serde_json::from_value(json!({
        "schema_version":1, "sources":sources, "assertions":[
            {"id":"release", "scope":{"kind":"model", "id":"lab/alpha"},
             "property":"release", "value":"alpha", "status":"supported",
             "conditions":[], "review":"accepted", "evidence":proof("model-doc")},
            {"id":"disable-thinking", "scope":{"kind":"interface", "id":"lab/api"},
             "property":"thinking.disabled", "value":true, "status":"supported",
             "conditions":[{"path":"thinking.budget", "op":"eq", "value":0}],
             "review":"accepted", "evidence":proof("interface-doc")}
        ]
    }))?;
    store::write_yaml(&root.join("data/assertions/core.yaml"), &assertions)?;
    Ok(catalogs)
}

#[test]
fn queries_use_explicit_typed_facts_and_keep_raw_record_output() -> Result<()> {
    let root = tempfile::tempdir()?;
    let catalogs = seed(root.path())?;
    for task in ["reranking", "text-ranking"] {
        let result = output(root.path(), &["query", "--task", task]);
        assert_eq!(ids(&result), [MODEL]);
        assert_eq!(result[0], serde_json::to_value(&catalogs[0].records[0])?);
    }
    for (task, expected) in [("feature-extraction", ZERO), ("future-task", UNKNOWN)] {
        assert_eq!(
            ids(&output(root.path(), &["query", "--task", task])),
            [expected]
        );
    }
    assert_eq!(
        ids(&output(root.path(), &["query", "--input", "text"])),
        [MODEL, HOSTED]
    );
    assert_eq!(
        ids(&output(
            root.path(),
            &["query", "--input", "image", "--output", "text"]
        )),
        [MODEL]
    );
    for modality in ["audio", "speech"] {
        assert_eq!(
            ids(&output(root.path(), &["query", "--output", modality])),
            [HOSTED]
        );
    }
    assert_eq!(
        ids(&output(root.path(), &["query", "--min-context", "0"])),
        [MODEL, ZERO]
    );
    assert_eq!(
        ids(&output(root.path(), &["query", "--min-context", "1"])),
        [MODEL]
    );
    for (capability, expected) in [
        ("reasoning", vec![MODEL]),
        ("tool-call", vec![MODEL, NATIVE]),
        ("structured-output", vec![ZERO]),
        ("open-weights", vec![MODEL, MIRROR]),
    ] {
        assert_eq!(
            ids(&output(root.path(), &["query", "--capability", capability])),
            expected
        );
    }
    assert_eq!(
        ids(&output(root.path(), &["query", "--identity", "lab/alpha"])),
        [MODEL, MIRROR, NATIVE]
    );
    assert_eq!(
        ids(&output(
            root.path(),
            &["query", "--identity", "lab/alpha", "--kind", "service"]
        )),
        [NATIVE]
    );
    assert_eq!(
        ids(&output(
            root.path(),
            &[
                "query",
                "ALPHA",
                "--kind",
                "model",
                "--source",
                "huggingface",
                "--limit",
                "1"
            ]
        )),
        [MODEL]
    );
    assert!(
        !run(root.path(), &["query", "--identity", "missing"])
            .status
            .success()
    );
    Ok(())
}

#[test]
fn inspect_exposes_evidence_bindings_relationships_and_sync_state() -> Result<()> {
    let root = tempfile::tempdir()?;
    seed(root.path())?;
    let inspected = output(root.path(), &["inspect", MODEL]);
    assert_eq!(inspected["record"]["id"], MODEL);
    assert_eq!(
        inspected["profile"]["limits"]["context_window"],
        json!({
            "value":4096, "fact":"context_window", "pointer":"/0/facts/context_window"
        })
    );
    assert_eq!(
        inspected["sync"],
        json!({"completed_at":"2026-10-16T00:01:00Z", "observed_in_last_sync":true})
    );
    assert_eq!(
        inspected["identities"],
        output(root.path(), &["identities", "--record", MODEL])
    );
    assert_eq!(
        inspected["identities"]["models"][0]["records"],
        json!([MODEL, MIRROR])
    );
    assert_eq!(inspected["identities"]["interfaces"], json!([]));
    assert!(
        inspected["identities"]["sources"]
            .get("model-doc")
            .is_some()
    );
    assert!(
        inspected["identities"]["sources"]
            .get("interface-doc")
            .is_none()
    );
    let relations = output(root.path(), &["relations", "--record", MODEL]);
    assert_eq!(relations["relationships"], inspected["relationships"]);
    assert_eq!(relations["total"], 3);
    let limited = output(
        root.path(),
        &["relations", "--record", MODEL, "--limit", "1"],
    );
    assert_eq!(limited["total"], 3);
    assert_eq!(limited["relationships"].as_array().unwrap().len(), 1);
    let native = output(root.path(), &["inspect", NATIVE]);
    assert_eq!(native["identities"]["interfaces"][0]["id"], "lab/api");
    assert_eq!(native["identities"]["models"], json!([]));
    assert_eq!(
        native["rules"]["scopes"],
        json!([{"kind":"interface", "id":"lab/api"}])
    );
    assert_eq!(native["rules"]["groups"].as_array().unwrap().len(), 1);
    assert_eq!(native["relationships"][0]["kind"], "serves");
    assert_eq!(native["relationships"][0]["to"], MIRROR);
    assert_eq!(native["relationships"][0]["target_present"], true);
    assert_eq!(
        native["rules"]["groups"][0]["resolution"]["state"],
        "unknown"
    );
    let unknown = output(root.path(), &["inspect", UNKNOWN]);
    assert_eq!(unknown["sync"]["observed_in_last_sync"], false);
    assert_eq!(unknown["rules"]["groups"], json!([]));
    assert_eq!(unknown["identities"]["sources"], json!({}));
    Ok(())
}

#[test]
fn rules_stay_with_explicit_scopes_including_unmapped_records() -> Result<()> {
    let root = tempfile::tempdir()?;
    seed(root.path())?;
    assert_eq!(
        output(root.path(), &["rules"])["groups"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for record in [MODEL, MIRROR] {
        let rules = output(root.path(), &["rules", "--record", record]);
        assert_eq!(rules["scopes"], json!([{"kind":"model", "id":"lab/alpha"}]));
        assert_eq!(rules["groups"].as_array().unwrap().len(), 1);
        assert_eq!(rules["groups"][0]["property"], "release");
        assert_eq!(
            output(
                root.path(),
                &[
                    "rules",
                    "--record",
                    record,
                    "--property",
                    "thinking.disabled"
                ]
            )["groups"],
            json!([])
        );
    }
    for record in [HOSTED, UNKNOWN] {
        let rules = output(root.path(), &["rules", "--record", record]);
        assert_eq!(rules["scopes"], json!([]));
        assert_eq!(rules["groups"], json!([]));
    }
    for (record, scope, kind) in [
        (HOSTED, "lab/api", "interface"),
        (NATIVE, "lab/alpha", "model"),
    ] {
        let unrelated = run(
            root.path(),
            &[
                "rules",
                "--record",
                record,
                "--scope",
                scope,
                "--scope-kind",
                kind,
            ],
        );
        assert!(!unrelated.status.success());
        assert!(String::from_utf8_lossy(&unrelated.stderr).contains("not explicitly bound"));
    }
    Ok(())
}

#[test]
fn rules_preserve_unknown_and_typed_context_and_reject_interface_conflicts() -> Result<()> {
    let root = tempfile::tempdir()?;
    seed(root.path())?;
    for selection in [
        vec!["--record", NATIVE],
        vec!["--scope", "lab/api", "--scope-kind", "interface"],
    ] {
        let mut args = vec!["rules"];
        args.extend(selection);
        let absent = output(root.path(), &args);
        assert_eq!(
            absent["scopes"],
            json!([{"kind":"interface", "id":"lab/api"}])
        );
        assert_eq!(absent["groups"].as_array().unwrap().len(), 1);
        assert_eq!(absent["groups"][0]["resolution"]["state"], "unknown");
        assert_eq!(
            absent["groups"][0]["candidates"][0]["condition_match"],
            json!({
                "state":"undetermined", "missing_paths":["thinking.budget"]
            })
        );
        for (context, expected) in [
            (r#"{"thinking.budget":0}"#, "known"),
            (r#"{"interface":"lab/api","thinking.budget":0}"#, "known"),
            (r#"{"thinking.budget":"0"}"#, "no_applicable_rule"),
            (r#"{"thinking":{"budget":0}}"#, "unknown"),
        ] {
            let mut contextual = args.clone();
            contextual.extend(["--context", context]);
            let result = output(root.path(), &contextual);
            assert_eq!(result["context"], serde_json::from_str::<Value>(context)?);
            assert_eq!(result["groups"][0]["resolution"]["state"], expected);
            if expected == "known" {
                assert_eq!(result["groups"][0]["resolution"]["value"], true);
            }
        }
        args.extend([
            "--context",
            r#"{"interface":"other/api","thinking.budget":0}"#,
        ]);
        let conflict = run(root.path(), &args);
        assert!(!conflict.status.success());
        assert!(String::from_utf8_lossy(&conflict.stderr).contains("context.interface conflicts"));
    }
    Ok(())
}

#[test]
fn evidence_archive_restores_exact_bytes_and_preserves_export() -> Result<()> {
    let root = tempfile::tempdir()?;
    let catalogs = seed(root.path())?;
    let snapshots = catalogs
        .iter()
        .flat_map(|catalog| &catalog.snapshots)
        .map(|snapshot| {
            let path = format!("snapshots/{}.json", snapshot.sha256);
            fs::read(root.path().join(".cache").join(&path)).map(|bytes| (path, bytes))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    success(root.path(), &["export"]);
    let export_path = root.path().join("dist/registry.json");
    let before = fs::read(&export_path)?;
    success(root.path(), &["evidence", "archive", "--output", "archive"]);
    for (path, bytes) in &snapshots {
        assert_eq!(&fs::read(root.path().join("archive").join(path))?, bytes);
    }
    fs::remove_dir_all(root.path().join(".cache"))?;
    success(root.path(), &["evidence", "restore", "--input", "archive"]);
    for (path, bytes) in &snapshots {
        assert_eq!(&fs::read(root.path().join(".cache").join(path))?, bytes);
    }
    success(root.path(), &["validate", "--evidence"]);
    success(root.path(), &["export"]);
    assert_eq!(before, fs::read(&export_path)?);
    let (path, bytes) = &snapshots[0];
    fs::write(root.path().join("archive").join(path), b"{}\n")?;
    let corrupt = run(root.path(), &["evidence", "restore", "--input", "archive"]);
    assert!(!corrupt.status.success());
    assert!(String::from_utf8_lossy(&corrupt.stderr).contains("SHA-256 mismatch"));
    assert_eq!(&fs::read(root.path().join(".cache").join(path))?, bytes);
    assert_eq!(before, fs::read(&export_path)?);
    Ok(())
}

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use anyhow::Result;
use model_metadata::{
    adapters,
    assertion::AssertionSet,
    documents::{self, DocumentCatalog, PageConfig, PageContent, PageDocument, PageSource},
    identity::IdentitySet,
    model::{
        Catalog, Fact, Snapshot, SyncFailure, SyncManifest, SyncScope, SyncStatus, pointer_segment,
    },
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
    for (source, pages) in [
        (
            "huggingface",
            vec![
                (
                    "https://huggingface.co/api/models/lab/alpha",
                    json!({
                        "id":"lab/alpha", "pipeline_tag":"text-ranking",
                        "cardData":{"base_model":["lab/base"]},
                        "fixture_facts":{
                            "input_modalities":["text","image"], "output_modalities":["text"],
                            "context_window":4096, "reasoning":true, "tool_call":true,
                            "structured_output":false, "open_weights":true
                        }
                    }),
                ),
                (
                    "https://huggingface.co/api/models/lab/zero",
                    json!({
                        "id":"lab/zero", "pipeline_tag":"feature-extraction",
                        "fixture_facts":{
                            "context_window":0, "reasoning":false, "tool_call":false,
                            "structured_output":true, "open_weights":false
                        }
                    }),
                ),
                (
                    "https://huggingface.co/api/models/lab/unknown",
                    json!({"id":"lab/unknown", "pipeline_tag":"future-task"}),
                ),
            ],
        ),
        (
            "modelsdev",
            vec![
                (
                    "https://models.dev/models.json",
                    json!({"lab/alpha":{
                        "id":"lab/alpha", "weights":[{"url":"https://huggingface.co/lab/alpha"}],
                        "open_weights":true
                    }}),
                ),
                (
                    "https://models.dev/catalog.json",
                    json!({"providers":{"native":{"id":"native", "models":{"alpha":{
                        "id":"alpha", "canonical_model_id":"lab/alpha", "tool_call":true
                    }}}}}),
                ),
            ],
        ),
        (
            "openrouter",
            vec![(
                "https://openrouter.ai/api/v1/models",
                json!({"data":[{
                    "id":"host/alpha", "hugging_face_id":"lab/alpha",
                    "architecture":{"input_modalities":["text"], "output_modalities":["speech"]}
                }]}),
            )],
        ),
    ] {
        let mut records = Vec::new();
        let mut snapshots = Vec::new();
        for (url, raw) in pages {
            let mut bytes = serde_json::to_vec_pretty(&raw)?;
            bytes.push(b'\n');
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            let snapshot = Snapshot {
                schema_version: 1,
                id: format!("{source}:{sha256}"),
                source: source.into(),
                url: url.into(),
                retrieved_at: "2026-10-16T00:00:00Z".into(),
                sha256,
                upstream_revision: None,
            };
            let mut normalized = adapters::normalize(&raw, &snapshot)?.unwrap();
            // Extra source-backed facts isolate projection semantics from adapter field coverage.
            if let Some(facts) = raw.get("fixture_facts").and_then(Value::as_object) {
                for (key, value) in facts {
                    normalized[0].facts.insert(
                        key.clone(),
                        Fact {
                            value: value.clone(),
                            pointer: format!("/fixture_facts/{}", pointer_segment(key)),
                            unit: (key == "context_window").then(|| "tokens".into()),
                        },
                    );
                }
            }
            records.extend(normalized);
            store::write_bytes(
                &root
                    .join(".cache/snapshots")
                    .join(format!("{}.json", snapshot.sha256)),
                &bytes,
            )?;
            snapshots.push(snapshot);
        }
        let mut returned_ids: Vec<_> = records
            .iter()
            .filter(|record| record.id != UNKNOWN)
            .map(|record| record.id.clone())
            .collect();
        returned_ids.sort();
        let mut snapshot_ids: Vec<_> = snapshots
            .iter()
            .map(|snapshot| snapshot.id.clone())
            .collect();
        snapshot_ids.sort();
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
                snapshot_ids,
                scope: Some(SyncScope::FullCatalog),
                status: Some(SyncStatus::Complete),
                last_successful_at: Some("2026-10-16T00:01:00Z".into()),
                ..Default::default()
            }),
            snapshots,
            records,
        };
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
        let mut expected = serde_json::to_value(&catalogs[0].records[0])?;
        expected["observation"] = json!({
            "attempted_at":"2026-10-16T00:01:00Z", "last_successful_at":"2026-10-16T00:01:00Z",
            "status":"observed"
        });
        assert_eq!(result[0], expected);
    }
    assert_eq!(
        ids(&output(
            root.path(),
            &["query", "--task", "feature-extraction"]
        )),
        [ZERO]
    );
    assert!(ids(&output(root.path(), &["query", "--task", "future-task"])).is_empty());
    assert_eq!(
        output(root.path(), &["query", UNKNOWN])[0]["facts"]["task"]["value"],
        "future-task"
    );
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
            "value":4096, "fact":"context_window", "pointer":"/fixture_facts/context_window"
        })
    );
    assert_eq!(
        inspected["observation"],
        json!({"attempted_at":"2026-10-16T00:01:00Z", "last_successful_at":"2026-10-16T00:01:00Z", "status":"observed"})
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
    assert_eq!(unknown["observation"]["status"], "missing");
    assert_eq!(unknown["rules"]["groups"], json!([]));
    assert_eq!(unknown["identities"]["sources"], json!({}));
    Ok(())
}

#[test]
fn observation_states_are_explicit_and_stale_catalog_facts_do_not_drive_queries() -> Result<()> {
    let root = tempfile::tempdir()?;
    let original = seed(root.path())?.remove(0);
    let previous = original.last_sync.as_ref().unwrap();
    let mut missing = previous.clone();
    missing.completed_at = "2026-10-17T00:01:00Z".into();
    missing.last_successful_at = Some(missing.completed_at.clone());
    missing.returned_ids = vec![ZERO.into()];
    missing.missing_ids = vec![MODEL.into(), UNKNOWN.into()];
    let mut not_requested = missing.clone();
    not_requested.scope = Some(SyncScope::SelectedModels {
        upstream_ids: vec!["lab/zero".into()],
    });
    not_requested.missing_ids.clear();
    let failed = SyncManifest {
        completed_at: "2026-10-18T00:01:00Z".into(),
        scope: Some(SyncScope::FullCatalog),
        status: Some(SyncStatus::Failed),
        errors: vec![SyncFailure {
            upstream_id: None,
            message: "HTTP 503".into(),
        }],
        last_successful_at: previous.last_successful_at.clone(),
        ..Default::default()
    };
    let mut legacy = previous.clone();
    legacy.scope = None;
    legacy.status = None;
    legacy.last_successful_at = None;
    for (state, manifest) in [
        ("observed", Some(previous.clone())),
        ("missing", Some(missing)),
        ("not_requested", Some(not_requested)),
        ("failed", Some(failed)),
        ("unknown", Some(legacy)),
        ("unknown", None),
    ] {
        let mut catalog = original.clone();
        catalog.last_sync = manifest;
        store::write_yaml(
            &root.path().join("data/catalogs/huggingface.yaml"),
            &catalog,
        )?;
        success(root.path(), &["validate", "--evidence"]);
        let expected = json!({
            "status":state,
            "attempted_at":catalog.last_sync.as_ref().map(|sync| &sync.completed_at),
            "last_successful_at":catalog.last_sync.as_ref().and_then(|sync| sync.last_successful_at.as_ref())
        });
        let inspected = output(root.path(), &["inspect", MODEL]);
        assert_eq!(inspected["observation"], expected);
        assert_eq!(inspected["sync"], serde_json::to_value(&catalog.last_sync)?);
        assert_eq!(
            inspected["record"],
            serde_json::to_value(&original.records[0])?
        );
        assert!(inspected["profile"]["lifecycle"].is_null());
        assert!(
            inspected["resolved_metadata"]["fields"]
                .get("lifecycle")
                .is_none()
        );
        for filter in ["observed", "missing", "not_requested", "failed", "unknown"] {
            let queried = output(root.path(), &["query", MODEL, "--observation", filter]);
            assert_eq!(
                ids(&queried),
                if state == filter { vec![MODEL] } else { vec![] }
            );
            if state == filter {
                assert_eq!(queried[0]["observation"], expected);
                assert_eq!(queried[0]["facts"], inspected["record"]["facts"]);
            }
        }
        let current = matches!(state, "observed" | "unknown");
        let resolved = output(root.path(), &["documents", "resolve", MODEL]);
        assert_eq!(resolved["observation"], state);
        let context = &resolved["fields"]["context_window"];
        assert_eq!(
            context["status"],
            if current { "selected" } else { "unavailable" }
        );
        assert_eq!(context["candidates"][0]["observation"], state);
        assert_eq!(context["candidates"][0]["fact"]["value"], 4096);
        assert_eq!(context.get("selected").is_some(), current);
        for filter in [
            ["--min-context", "1"],
            ["--capability", "reasoning"],
            ["--input", "image"],
            ["--task", "text-ranking"],
            ["--task", "reranking"],
        ] {
            let queried = output(root.path(), &["query", MODEL, filter[0], filter[1]]);
            assert_eq!(ids(&queried), if current { vec![MODEL] } else { vec![] });
        }
        success(root.path(), &["export"]);
        let exported: Value =
            serde_json::from_slice(&fs::read(root.path().join("dist/registry.json"))?)?;
        assert_eq!(exported["observations"][MODEL], expected);
        assert_eq!(exported["resolved_metadata"][MODEL], resolved);
    }
    Ok(())
}

#[test]
fn current_official_claims_remain_usable_when_the_bound_catalog_record_is_missing() -> Result<()> {
    let root = tempfile::tempdir()?;
    let mut catalogs = seed(root.path())?;
    let sync = catalogs[0].last_sync.as_mut().unwrap();
    sync.returned_ids = vec![ZERO.into()];
    sync.missing_ids = vec![MODEL.into(), UNKNOWN.into()];
    store::write_yaml(
        &root.path().join("data/catalogs/huggingface.yaml"),
        &catalogs[0],
    )?;

    let source: PageSource = serde_json::from_value(json!({
        "id":"openai-gpt41-model", "vendor":"openai", "kind":"model_page",
        "url":"https://developers.openai.com/api/docs/models/gpt-4.1", "format":"html",
        "records":[MIRROR]
    }))?;
    let bytes = br"<html><head><title>GPT-4.1</title></head><body><main><h1>GPT-4.1</h1>
        <p>GPT-4.1 excels at instruction following and tool calling. It performs without a reasoning step.</p>
        <div>8,192 context window</div><div>4,096 max output tokens</div>
        <section><div>Modalities</div><div><div>Text</div><div>Input and output</div></div></section>
        </main></body></html>";
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let snapshot = Snapshot {
        schema_version: 1,
        id: format!("official-openai:{sha256}"),
        source: "official-openai".into(),
        url: source.url.clone(),
        retrieved_at: "2026-10-17T00:00:00Z".into(),
        sha256,
        upstream_revision: None,
    };
    let documents = DocumentCatalog {
        schema_version: 1,
        documents: vec![PageDocument {
            source: source.clone(),
            checked_at: "2026-10-17T00:01:00Z".into(),
            error: None,
            error_kind: None,
            failed_snapshot: None,
            content: Some(PageContent {
                extraction: documents::extract(&source, bytes)?,
                snapshot: snapshot.clone(),
                content_type: Some("text/html".into()),
            }),
        }],
    };
    store::write_yaml(
        &root.path().join("config/official-pages.yaml"),
        &PageConfig {
            schema_version: 1,
            pages: vec![source],
        },
    )?;
    store::write_yaml(
        &root.path().join("data/documents/official.yaml"),
        &documents,
    )?;
    store::write_bytes(
        &root
            .path()
            .join(format!(".cache/documents/{}.bin", snapshot.sha256)),
        bytes,
    )?;
    success(root.path(), &["validate", "--evidence"]);

    let resolved = output(root.path(), &["documents", "resolve", MODEL]);
    assert_eq!(resolved["observation"], "missing");
    for (property, value) in [
        ("context_window", json!(8192)),
        ("input_modalities", json!(["text"])),
        ("reasoning", json!(false)),
    ] {
        let field = &resolved["fields"][property];
        assert_eq!(field["status"], "selected");
        let selected = &field["candidates"][field["selected"].as_u64().unwrap() as usize];
        assert_eq!(selected["type"], "document");
        assert_eq!(selected["claim"]["record"], MIRROR);
        assert_eq!(selected["claim"]["value"], value);
        let retained = field["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["type"] == "catalog")
            .unwrap();
        assert_eq!(retained["observation"], "missing");
    }
    for filter in [["--min-context", "8000"], ["--input", "text"]] {
        let queried = output(
            root.path(),
            &[
                "query",
                MODEL,
                filter[0],
                filter[1],
                "--observation",
                "missing",
            ],
        );
        assert_eq!(ids(&queried), [MODEL]);
        assert_eq!(queried[0]["facts"]["context_window"]["value"], 4096);
    }
    assert_eq!(resolved["fields"]["open_weights"]["status"], "unavailable");
    assert_eq!(
        ids(&output(
            root.path(),
            &["query", MODEL, "--capability", "open-weights"]
        )),
        Vec::<&str>::new()
    );
    assert_eq!(
        ids(&output(
            root.path(),
            &["query", MODEL, "--capability", "reasoning"]
        )),
        Vec::<&str>::new()
    );
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

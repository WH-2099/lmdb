use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use anyhow::Result;
use model_metadata::{
    documents::{self, DocumentCatalog, PageConfig, PageContent, PageDocument, PageSource},
    model::{Catalog, Snapshot},
    store,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MODEL: &str = "modelsdev:model:openai/gpt-4.1";
const SIMILAR: &str = "modelsdev:model:openai/gpt-4.1-mini";
const URL: &str = "https://developers.openai.com/api/docs/models/gpt-4.1";
const RELEASE: &str = "https://openai.com/index/gpt-4-1/";
const RAW: &[u8] = b"{\n  \"model\": { \"context_window\": 4096 },\n  \"similar\": { \"context_window\": 1047576 }\n}\n";
const HTML: &[u8] = br#"<!doctype html>
<html><head><title>GPT-4.1</title>
<link rel="canonical" href="https://developers.openai.com/api/docs/models/gpt-4.1">
</head><body><main><h1>GPT-4.1</h1>
<p>GPT-4.1 excels at instruction following and tool calling. It performs without a reasoning step.</p>
<div>1,047,576 context window</div><div>32,768 max output tokens</div>
<a href="https://openai.com/index/gpt-4-1/">Release announcement</a>
</main></body></html>
"#;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lmdb"))
        .env_clear()
        .env("HTTP_PROXY", "http://127.0.0.1:0")
        .env("HTTPS_PROXY", "http://127.0.0.1:0")
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run lmdb")
}

fn success(root: &Path, args: &[&str]) -> Vec<u8> {
    let result = run(root, args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

fn output(root: &Path, args: &[&str]) -> Value {
    serde_json::from_slice(&success(root, args)).expect("JSON command output")
}

fn snapshot(source: &str, url: &str, bytes: &[u8]) -> Snapshot {
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    Snapshot {
        schema_version: 1,
        id: format!("{source}:{sha256}"),
        source: source.into(),
        url: url.into(),
        retrieved_at: "2026-10-16T00:00:00Z".into(),
        sha256,
        upstream_revision: None,
    }
}

fn seed(root: &Path) -> Result<(Catalog, DocumentCatalog)> {
    let raw_snapshot = snapshot("modelsdev", "https://models.dev/models.json", RAW);
    let records: Vec<_> = [
        ("openai/gpt-4.1", "/model", 4096),
        ("openai/gpt-4.1-mini", "/similar", 1047576),
    ].into_iter().map(|(upstream_id, pointer, context)| json!({
        "schema_version":1, "kind":"model", "id":format!("modelsdev:model:{upstream_id}"),
        "source":"modelsdev", "upstream_id":upstream_id, "name":upstream_id,
        "origin":{"snapshot":raw_snapshot.id, "pointer":pointer},
        "facts":{"context_window":{"value":context, "unit":"tokens", "pointer":format!("{pointer}/context_window")}}
    })).collect();
    let catalog: Catalog = serde_json::from_value(json!({
        "schema_version":1, "source":"modelsdev", "snapshots":[raw_snapshot], "records":records
    }))?;
    store::write_yaml(&root.join("data/catalogs/modelsdev.yaml"), &catalog)?;
    store::write_bytes(
        &root.join(format!(".cache/snapshots/{}.json", raw_snapshot.sha256)),
        RAW,
    )?;
    let source: PageSource = serde_json::from_value(json!({
        "id":"openai-gpt41-model", "vendor":"openai", "kind":"model_page",
        "url":URL, "format":"html", "records":[MODEL]
    }))?;
    store::write_yaml(
        &root.join("config/official-pages.yaml"),
        &PageConfig {
            schema_version: 1,
            pages: vec![source.clone()],
        },
    )?;
    let content = PageContent {
        snapshot: snapshot("official-openai", URL, HTML),
        content_type: Some("text/html; charset=utf-8".into()),
        extraction: documents::extract(&source, HTML)?,
    };
    store::write_bytes(
        &root.join(format!(".cache/documents/{}.bin", content.snapshot.sha256)),
        HTML,
    )?;
    let documents = DocumentCatalog {
        schema_version: 1,
        documents: vec![PageDocument {
            source,
            checked_at: "2026-10-16T00:01:00Z".into(),
            error: None,
            content: Some(content),
        }],
    };
    store::write_yaml(&root.join("data/documents/official.yaml"), &documents)?;
    Ok((catalog, documents))
}

#[test]
fn official_resolution_drives_queries_and_preserves_raw_records() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (catalog, documents) = seed(root.path())?;
    let catalog_path = root.path().join("data/catalogs/modelsdev.yaml");
    let original_catalog = fs::read(&catalog_path)?;
    let raw_record = serde_json::to_value(&catalog.records[0])?;

    let queried = output(root.path(), &["query", "--min-context", "100000"]);
    let found = queried
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == MODEL)
        .expect("official context includes the model in the query");
    assert_eq!(found, &raw_record);
    assert_eq!(found["facts"]["context_window"]["value"], 4096);

    let resolved = output(root.path(), &["documents", "resolve", MODEL]);
    assert_eq!(resolved["record"], MODEL);
    let context = &resolved["fields"]["context_window"];
    assert_eq!(context["status"], "selected");
    let selected = &context["candidates"][context["selected"].as_u64().unwrap() as usize];
    assert_eq!(selected["type"], "document");
    assert_eq!(selected["document"], "openai-gpt41-model");
    assert_eq!(selected["kind"], "model_page");
    assert_eq!(selected["claim"]["value"], 1047576);
    assert_eq!(selected["claim"]["scope"]["kind"], "model");
    assert_eq!(selected["stale"], false);
    assert_eq!(
        selected["snapshot"],
        serde_json::to_value(&documents.documents[0].content.as_ref().unwrap().snapshot)?
    );
    let claim = documents.documents[0]
        .content
        .as_ref()
        .unwrap()
        .extraction
        .claims
        .iter()
        .find(|claim| claim.property == "context_window")
        .unwrap();
    assert_eq!(selected["claim"], serde_json::to_value(claim)?);
    let raw = context["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["type"] == "catalog")
        .expect("original catalog evidence remains available");
    assert_eq!(raw["origin"], raw_record["origin"]);
    assert_eq!(raw["fact"], raw_record["facts"]["context_window"]);

    let inspected = output(root.path(), &["inspect", MODEL]);
    assert_eq!(inspected["record"], raw_record);
    assert_eq!(inspected["resolved_metadata"], resolved);
    success(root.path(), &["export"]);
    let exported: Value =
        serde_json::from_slice(&fs::read(root.path().join("dist/registry.json"))?)?;
    assert_eq!(exported["resolved_metadata"][MODEL], resolved);
    assert_eq!(
        exported["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["id"] == MODEL)
            .unwrap(),
        &raw_record
    );
    assert_eq!(fs::read(catalog_path)?, original_catalog);
    Ok(())
}

#[test]
fn conflicting_official_claims_block_catalog_fallback_in_queries() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_, mut documents) = seed(root.path())?;
    // Synthetic saved claims isolate conflict handling; replay is tested separately.
    let mut alternate = documents.documents[0].clone();
    alternate.source.id = "openai-gpt41-alternate".into();
    alternate.source.url = format!("{URL}?revision=alternate");
    let content = alternate.content.as_mut().unwrap();
    content.snapshot.url = alternate.source.url.clone();
    content
        .extraction
        .claims
        .iter_mut()
        .find(|claim| claim.property == "context_window")
        .unwrap()
        .value = json!(2097152);
    documents.documents.push(alternate);
    store::write_yaml(
        &root.path().join("config/official-pages.yaml"),
        &PageConfig {
            schema_version: 1,
            pages: documents
                .documents
                .iter()
                .map(|doc| doc.source.clone())
                .collect(),
        },
    )?;
    store::write_yaml(
        &root.path().join("data/documents/official.yaml"),
        &documents,
    )?;
    success(root.path(), &["validate"]);

    let resolved = output(root.path(), &["documents", "resolve", MODEL]);
    let context = &resolved["fields"]["context_window"];
    assert_eq!(context["status"], "conflict");
    assert!(context.get("selected").is_none());
    assert_eq!(context["candidates"].as_array().unwrap().len(), 3);
    let queried = output(root.path(), &["query", "--min-context", "1000"]);
    assert!(
        queried
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["id"] != MODEL)
    );
    assert!(
        queried
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["id"] == SIMILAR)
    );
    let unfiltered = output(root.path(), &["query"]);
    assert!(unfiltered.as_array().unwrap().iter().any(|record| {
        record["id"] == MODEL && record["facts"]["context_window"]["value"] == 4096
    }));
    Ok(())
}

#[test]
fn reparse_migrates_cached_extractions_and_never_partially_writes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_, mut expected) = seed(root.path())?;
    expected.documents[0].checked_at = "2026-10-17T00:00:00Z".into();
    expected.documents[0].error = Some("HTTP 503 from last refresh".into());
    let mut second = expected.documents[0].clone();
    second.source.id = "openai-gpt41-blog".into();
    second.source.url = RELEASE.into();
    second.source.kind = documents::DocumentKind::ReleaseBlog;
    let second_body = br"<html><head><title>GPT-4.1 release</title></head><body><article><h1>GPT-4.1</h1><p>Introducing GPT-4.1.</p></article></body></html>";
    let second_content = second.content.as_mut().unwrap();
    second_content.snapshot = snapshot("official-openai", RELEASE, second_body);
    second_content.extraction = documents::extract(&second.source, second_body)?;
    let second_path = root.path().join(format!(
        ".cache/documents/{}.bin",
        second_content.snapshot.sha256
    ));
    store::write_bytes(&second_path, second_body)?;
    expected.documents.push(second);
    store::write_yaml(
        &root.path().join("config/official-pages.yaml"),
        &PageConfig {
            schema_version: 1,
            pages: expected
                .documents
                .iter()
                .map(|doc| doc.source.clone())
                .collect(),
        },
    )?;

    let mut legacy = serde_json::to_value(&expected)?;
    for document in legacy["documents"].as_array_mut().unwrap() {
        let extraction = document["content"]["extraction"].as_object_mut().unwrap();
        extraction.insert("extractor_version".into(), json!(1));
        extraction.remove("passages");
        for claim in extraction["claims"].as_array_mut().unwrap() {
            claim.as_object_mut().unwrap().remove("scope");
            if claim["property"] == "context_window" {
                claim["value"] = json!(999);
            }
        }
    }
    let path = root.path().join("data/documents/official.yaml");
    store::write_yaml(&path, &legacy)?;
    let old_bytes = fs::read(&path)?;
    success(root.path(), &["documents", "reparse"]);
    let reparsed = documents::load(root.path())?;
    assert_eq!(reparsed, expected);
    let content = reparsed.documents[0].content.as_ref().unwrap();
    assert_eq!(content.extraction.extractor_version, 2);
    assert!(!content.extraction.passages.is_empty());
    assert!(content.extraction.claims.iter().any(|claim| {
        claim.property == "context_window"
            && claim.value == json!(1047576)
            && claim.scope == documents::ClaimScope::Model
    }));
    success(root.path(), &["validate", "--evidence"]);

    fs::write(&path, &old_bytes)?;
    fs::remove_file(&second_path)?;
    let missing = run(root.path(), &["documents", "reparse"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("open document"));
    assert_eq!(fs::read(&path)?, old_bytes);
    fs::write(second_path, b"corrupt")?;
    let corrupt = run(root.path(), &["documents", "reparse"]);
    assert!(!corrupt.status.success());
    assert!(String::from_utf8_lossy(&corrupt.stderr).contains("document SHA-256 mismatch"));
    assert_eq!(fs::read(path)?, old_bytes);
    Ok(())
}

#[test]
fn partial_sync_rejects_changed_or_removed_unselected_sources_before_fetching() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_, documents) = seed(root.path())?;
    let path = root.path().join("data/documents/official.yaml");
    let before = fs::read(&path)?;
    let selected: PageSource = serde_json::from_value(json!({
        "id":"qwen-test", "vendor":"qwen", "kind":"release_blog",
        "url":"https://qwenlm.github.io/blog/qwen3/", "format":"html", "records":[MODEL]
    }))?;
    for keep_changed_source in [true, false] {
        let mut pages = vec![selected.clone()];
        if keep_changed_source {
            let mut changed = documents.documents[0].source.clone();
            changed.url = format!("{URL}?revision=changed");
            pages.push(changed);
        }
        store::write_yaml(
            &root.path().join("config/official-pages.yaml"),
            &PageConfig {
                schema_version: 1,
                pages,
            },
        )?;
        let result = run(root.path(), &["documents", "sync", "--vendor", "qwen"]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("full sync"));
        assert_eq!(fs::read(&path)?, before);
    }
    Ok(())
}

#[test]
fn document_commands_preserve_source_evidence_and_exact_record_bindings() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_, documents) = seed(root.path())?;
    let listed = output(
        root.path(),
        &["documents", "list", "--vendor", "openai", "--record", MODEL],
    );
    assert_eq!(listed["sources"][0]["url"], URL);
    assert_eq!(
        listed["documents"],
        serde_json::to_value(&documents.documents)?
    );
    let extraction = &listed["documents"][0]["content"]["extraction"];
    assert_eq!(extraction["title"]["value"], "GPT-4.1");
    assert_eq!(extraction["canonical_url"]["value"], URL);
    assert!(
        extraction["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| link["url"] == RELEASE)
    );
    let claim = extraction["claims"]
        .as_array()
        .unwrap()
        .iter()
        .find(|claim| claim["property"] == "context_window")
        .expect("synthetic context claim");
    assert_eq!(claim["record"], MODEL);
    assert_eq!(claim["value"], 1047576);
    assert_eq!(claim["unit"], "tokens");
    assert_eq!(claim["evidence"]["excerpt"], "1,047,576 context window");
    assert!(!claim["evidence"]["locator"].as_str().unwrap().is_empty());

    let comparisons = output(root.path(), &["documents", "check", "--record", MODEL]);
    let context = comparisons
        .as_array()
        .unwrap()
        .iter()
        .find(|comparison| comparison["property"] == "context_window")
        .expect("context comparison");
    assert_eq!(context["record"], MODEL);
    assert_eq!(context["status"], "difference");
    assert_eq!(context["official"]["value"], 1047576);
    assert_eq!(context["official_snapshot"]["url"], URL);
    assert_eq!(context["catalog"]["value"], 4096);
    assert_eq!(context["catalog_origin"]["pointer"], "/model");
    assert_eq!(context["official_stale"], false);
    let inspected = output(root.path(), &["inspect", MODEL]);
    assert_eq!(inspected["official_sources"], listed["sources"]);
    assert_eq!(inspected["documents"], listed["documents"]);
    assert_eq!(inspected["document_comparisons"], comparisons);
    success(root.path(), &["export"]);
    let exported: Value =
        serde_json::from_slice(&fs::read(root.path().join("dist/registry.json"))?)?;
    assert_eq!(exported["official_sources"], listed["sources"]);
    assert_eq!(exported["documents"], listed["documents"]);
    assert_eq!(exported["document_comparisons"], comparisons);

    assert_eq!(
        output(root.path(), &["documents", "list", "--record", SIMILAR]),
        json!({"sources":[], "documents":[]})
    );
    assert_eq!(
        output(root.path(), &["documents", "check", "--record", SIMILAR]),
        json!([])
    );
    let similar = output(root.path(), &["inspect", SIMILAR]);
    for field in ["official_sources", "documents", "document_comparisons"] {
        assert_eq!(similar[field], json!([]));
    }
    Ok(())
}

#[test]
fn evidence_validation_replays_claims_and_rejects_changed_page_bytes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_, original) = seed(root.path())?;
    success(root.path(), &["validate", "--evidence"]);
    let mut changed = original.clone();
    let content = changed.documents[0].content.as_mut().unwrap();
    content
        .extraction
        .claims
        .iter_mut()
        .find(|claim| claim.property == "context_window")
        .expect("synthetic context claim")
        .value = json!(2048);
    store::write_yaml(&root.path().join("data/documents/official.yaml"), &changed)?;
    success(root.path(), &["validate"]);
    let mismatch = run(root.path(), &["validate", "--evidence"]);
    assert!(!mismatch.status.success());
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("differs from snapshot replay"));

    store::write_yaml(&root.path().join("data/documents/official.yaml"), &original)?;
    let hash = &original.documents[0]
        .content
        .as_ref()
        .unwrap()
        .snapshot
        .sha256;
    fs::write(
        root.path().join(format!(".cache/documents/{hash}.bin")),
        b"<html>changed</html>",
    )?;
    let corrupt = run(root.path(), &["validate", "--evidence"]);
    assert!(!corrupt.status.success());
    assert!(String::from_utf8_lossy(&corrupt.stderr).contains("document SHA-256 mismatch"));
    Ok(())
}

#[test]
fn archive_restores_json_and_document_evidence_into_an_empty_cache() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (catalog, documents) = seed(root.path())?;
    let raw_path = format!("snapshots/{}.json", catalog.snapshots[0].sha256);
    let document_path = format!(
        "documents/{}.bin",
        documents.documents[0]
            .content
            .as_ref()
            .unwrap()
            .snapshot
            .sha256
    );
    success(root.path(), &["export"]);
    let export = fs::read(root.path().join("dist/registry.json"))?;
    success(root.path(), &["evidence", "archive", "--output", "archive"]);
    for (path, expected) in [(&raw_path, RAW), (&document_path, HTML)] {
        assert_eq!(fs::read(root.path().join("archive").join(path))?, expected);
    }
    fs::remove_dir_all(root.path().join(".cache"))?;
    success(root.path(), &["evidence", "restore", "--input", "archive"]);
    for (path, expected) in [(&raw_path, RAW), (&document_path, HTML)] {
        assert_eq!(fs::read(root.path().join(".cache").join(path))?, expected);
    }
    success(root.path(), &["validate", "--evidence"]);
    success(root.path(), &["export"]);
    assert_eq!(export, fs::read(root.path().join("dist/registry.json"))?);

    fs::write(root.path().join("archive").join(&document_path), b"corrupt")?;
    fs::remove_dir_all(root.path().join(".cache"))?;
    let result = run(root.path(), &["evidence", "restore", "--input", "archive"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("document SHA-256 mismatch"));
    assert!(!root.path().join(".cache").exists());
    assert_eq!(export, fs::read(root.path().join("dist/registry.json"))?);
    Ok(())
}

#[test]
fn fetch_errors_remain_visible_with_retained_or_absent_content() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (_, mut documents) = seed(root.path())?;
    documents.documents[0].checked_at = "2026-10-17T00:00:00Z".into();
    documents.documents[0].error = Some("HTTP 503 Service Unavailable".into());
    store::write_yaml(
        &root.path().join("data/documents/official.yaml"),
        &documents,
    )?;
    success(root.path(), &["validate", "--evidence"]);
    let listed = output(root.path(), &["documents", "list"]);
    assert_eq!(
        listed["documents"][0]["error"],
        "HTTP 503 Service Unavailable"
    );
    assert_eq!(listed["documents"][0]["checked_at"], "2026-10-17T00:00:00Z");
    assert_eq!(
        listed["documents"][0]["content"]["snapshot"]["retrieved_at"],
        "2026-10-16T00:00:00Z"
    );
    let comparisons = output(root.path(), &["documents", "check", "--record", MODEL]);
    assert!(!comparisons.as_array().unwrap().is_empty());
    assert!(
        comparisons
            .as_array()
            .unwrap()
            .iter()
            .all(|comparison| comparison["official_stale"] == true
                && comparison["source_error"] == "HTTP 503 Service Unavailable"
                && comparison["status"] == "not_comparable")
    );
    let inspected = output(root.path(), &["inspect", MODEL]);
    assert_eq!(inspected["documents"], listed["documents"]);
    assert_eq!(inspected["document_comparisons"], comparisons);
    let resolved = output(root.path(), &["documents", "resolve", MODEL]);
    assert_eq!(resolved["fields"]["context_window"]["status"], "qualified");
    let queried = output(root.path(), &["query", "--min-context", "1"]);
    assert!(
        queried
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["id"] != MODEL)
    );
    success(root.path(), &["export"]);
    let exported: Value =
        serde_json::from_slice(&fs::read(root.path().join("dist/registry.json"))?)?;
    assert_eq!(exported["documents"], listed["documents"]);
    assert_eq!(exported["document_comparisons"], comparisons);

    documents.documents[0].content = None;
    store::write_yaml(
        &root.path().join("data/documents/official.yaml"),
        &documents,
    )?;
    let listed = output(root.path(), &["documents", "list"]);
    assert_eq!(
        listed["documents"][0]["error"],
        "HTTP 503 Service Unavailable"
    );
    assert!(listed["documents"][0].get("content").is_none());
    assert_eq!(
        output(root.path(), &["documents", "check", "--record", MODEL]),
        json!([])
    );
    let inspected = output(root.path(), &["inspect", MODEL]);
    assert_eq!(inspected["documents"], listed["documents"]);
    assert_eq!(inspected["document_comparisons"], json!([]));
    success(root.path(), &["export"]);
    let exported: Value =
        serde_json::from_slice(&fs::read(root.path().join("dist/registry.json"))?)?;
    assert_eq!(exported["documents"], listed["documents"]);
    assert_eq!(exported["document_comparisons"], json!([]));
    Ok(())
}

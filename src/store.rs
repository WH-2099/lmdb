use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail, ensure};
use granit_parser::{Event, Parser};
use serde::{Serialize, de::DeserializeOwned};
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy, NonFiniteFloatPolicy};

use crate::model::Catalog;

const MAX_YAML_BYTES: u64 = 128 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn read_yaml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_YAML_BYTES,
        "{} exceeds the 128 MiB YAML limit",
        path.display()
    );
    let mut text = String::new();
    file.take(MAX_YAML_BYTES + 1)
        .read_to_string(&mut text)
        .with_context(|| format!("read UTF-8 YAML from {}", path.display()))?;
    ensure!(
        text.len() as u64 <= MAX_YAML_BYTES,
        "{} exceeds the 128 MiB YAML limit",
        path.display()
    );

    let options = serde_saphyr::options! {
        strict_booleans: true,
        no_schema: true,
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        reject_unsupported_tags: true,
        non_finite_float_policy: NonFiniteFloatPolicy::Reject,
        budget: serde_saphyr::budget! {
            max_documents: 1,
            max_anchors: 0,
            max_aliases: 0,
            max_depth: 32,
            max_nodes: 8_000_000,
            max_events: 32_000_000,
            max_total_scalar_bytes: MAX_YAML_BYTES as usize,
        },
    };
    let value: serde_json::Value = serde_saphyr::from_str_with_options(&text, options)
        .with_context(|| format!("parse {}", path.display()))?;

    // Serde accepts built-in YAML tags, so inspect parser events before discarding their syntax.
    for event in Parser::new_from_str(&text) {
        let (event, span) = event.with_context(|| format!("parse {}", path.display()))?;
        if matches!(
            event,
            Event::Scalar(_, _, _, Some(_))
                | Event::SequenceStart(_, _, Some(_))
                | Event::MappingStart(_, _, Some(_))
        ) {
            let marker = span.tag_start.unwrap_or(span.start);
            bail!(
                "{}:{}:{}: explicit YAML tags are not allowed",
                path.display(),
                marker.line(),
                marker.col() + 1
            );
        }
    }
    serde_json::from_value(value).with_context(|| format!("decode {}", path.display()))
}

pub fn write_yaml<T: Serialize>(path: &Path, value: &T) -> Result<bool> {
    let text =
        serde_saphyr::to_string(value).with_context(|| format!("serialize {}", path.display()))?;
    ensure!(
        text.len() as u64 <= MAX_YAML_BYTES,
        "{} exceeds the 128 MiB YAML limit",
        path.display()
    );
    write_bytes(path, text.as_bytes())
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<bool> {
    let existing = match fs::metadata(path) {
        Ok(metadata) => {
            ensure!(metadata.is_file(), "{} is not a file", path.display());
            if metadata.len() == bytes.len() as u64 && fs::read(path)? == bytes {
                return Ok(false);
            }
            Some(metadata)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().context("output path must name a file")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;

    let (temporary_path, mut temporary) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut name = OsString::from(".");
        name.push(filename);
        name.push(format!(".{}.{sequence}.tmp", std::process::id()));
        let temporary_path = parent.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => break (temporary_path, file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("create {}", temporary_path.display()));
            }
        }
    };
    let result = (|| {
        if let Some(metadata) = existing {
            temporary.set_permissions(metadata.permissions())?;
        }
        temporary.write_all(bytes)?;
        temporary.sync_all()
    })();
    drop(temporary);
    let result = result.and_then(|()| fs::rename(&temporary_path, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result.with_context(|| format!("atomically replace {}", path.display()))?;
    Ok(true)
}

pub fn load_catalogs(root: &Path) -> Result<Vec<Catalog>> {
    let directory = root.join("data/catalogs");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("read {}", directory.display()));
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "yaml")
        {
            paths.push(path);
        }
    }
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let catalog: Catalog = read_yaml(path)?;
            let expected = format!("{}.yaml", catalog.source);
            ensure!(
                path.file_name()
                    .is_some_and(|name| name == expected.as_str()),
                "{}: catalog filename must be {expected}",
                path.display()
            );
            Ok(catalog)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::model::{Fact, Origin, Record, RecordKind, SCHEMA_VERSION};

    fn catalog(source: &str) -> Catalog {
        Catalog {
            schema_version: SCHEMA_VERSION,
            source: source.into(),
            snapshots: Vec::new(),
            records: vec![Record {
                schema_version: SCHEMA_VERSION,
                kind: RecordKind::Model,
                id: format!("{source}/model"),
                source: source.into(),
                upstream_id: "model".into(),
                name: "Model".into(),
                model: None,
                origin: Origin {
                    snapshot: "snapshot".into(),
                    pointer: "/data/0".into(),
                },
                facts: BTreeMap::from([(
                    "context_length".into(),
                    Fact {
                        value: 128_000.into(),
                        pointer: "/data/0/context_length".into(),
                        unit: Some("tokens".into()),
                    },
                )]),
            }],
        }
    }

    #[test]
    fn rejects_ambiguous_and_resource_amplifying_yaml() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let path = temporary.path().join("input.yaml");
        for text in [
            "a: 1\na: 2",
            "a: {b: 1, b: 2}",
            "a: 1\n---\na: 2",
            "a: &item 1\nb: *item",
            "a: &unused 1",
            "a: !custom value",
            "a: !!str value",
            "a: !!binary dmFsdWU=",
            "a: !!seq [1, 2]",
            "a: !!map {b: 1}",
            "a: !include /etc/passwd",
            "a: .nan",
            "a: .inf",
            "a: 1e999",
            "1: value",
            "true: value",
            "[a, b]: value",
            "a: {<<: {b: 1}}",
        ] {
            fs::write(&path, text)?;
            assert!(read_yaml::<serde_json::Value>(&path).is_err(), "{text}");
        }
        fs::write(&path, format!("{}0{}", "[".repeat(33), "]".repeat(33)))?;
        assert!(read_yaml::<serde_json::Value>(&path).is_err());
        fs::write(&path, [0xff, 0xfe])?;
        assert!(read_yaml::<serde_json::Value>(&path).is_err());
        File::create(&path)?.set_len(MAX_YAML_BYTES + 1)?;
        assert!(read_yaml::<serde_json::Value>(&path).is_err());
        Ok(())
    }

    #[test]
    fn accepts_tag_text_and_rejects_unknown_catalog_fields() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let path = temporary.path().join("input.yaml");
        fs::write(&path, "text: |\n  !!str is text\nquoted: '!custom' # !tag")?;
        let value: serde_json::Value = read_yaml(&path)?;
        assert_eq!(value["quoted"], "!custom");
        fs::write(
            &path,
            "schema_version: 1\nsource: test\nsnapshots: []\nrecords: []\nunexpected: true",
        )?;
        assert!(read_yaml::<Catalog>(&path).is_err());
        Ok(())
    }

    #[test]
    fn repeated_catalog_write_keeps_bytes_and_modified_time() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let path = temporary.path().join("catalog.yaml");
        let original = catalog("test");
        assert!(write_yaml(&path, &original)?);
        let bytes = fs::read(&path)?;
        let modified = fs::metadata(&path)?.modified()?;
        assert!(!write_yaml(&path, &original)?);
        assert_eq!(bytes, fs::read(&path)?);
        assert_eq!(modified, fs::metadata(&path)?.modified()?);
        assert_eq!(original, read_yaml::<Catalog>(&path)?);
        assert!(!write_bytes(&path, &bytes)?);
        let mut updated = original;
        updated.records[0].name = "Updated model".into();
        assert!(write_yaml(&path, &updated)?);
        assert_eq!(updated, read_yaml::<Catalog>(&path)?);
        assert_eq!(fs::read_dir(temporary.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn failures_preserve_existing_file() -> Result<()> {
        struct Failing;
        impl Serialize for Failing {
            fn serialize<S: serde::Serializer>(
                &self,
                _: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("serialization failed"))
            }
        }

        let temporary = tempfile::tempdir()?;
        let path = temporary.path().join("catalog.yaml");
        write_yaml(&path, &catalog("test"))?;
        let bytes = fs::read(&path)?;
        assert!(write_yaml(&path, &Failing).is_err());
        assert!(write_yaml(&path.join("child.yaml"), &catalog("other")).is_err());
        assert_eq!(fs::read(&path)?, bytes);
        assert_eq!(fs::read_dir(temporary.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn catalogs_are_loaded_by_sorted_yaml_filename() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        assert!(load_catalogs(temporary.path())?.is_empty());
        let directory = temporary.path().join("data/catalogs");
        write_yaml(&directory.join("b.yaml"), &catalog("b"))?;
        write_yaml(&directory.join("a.yaml"), &catalog("a"))?;
        fs::write(directory.join("ignored.yml"), "invalid: [")?;
        fs::write(directory.join("ignored.json"), "not JSON")?;
        let catalogs = load_catalogs(temporary.path())?;
        assert_eq!(
            catalogs
                .iter()
                .map(|item| item.source.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        let alias = directory.join("alias.yaml");
        write_yaml(&alias, &catalog("huggingface"))?;
        let error = load_catalogs(temporary.path()).unwrap_err().to_string();
        assert!(
            error.contains("catalog filename must be huggingface.yaml"),
            "{error}"
        );
        fs::rename(alias, directory.join("huggingface.yaml"))?;
        assert_eq!(load_catalogs(temporary.path())?.len(), 3);
        fs::write(directory.join("c.yaml"), "invalid: [")?;
        assert!(load_catalogs(temporary.path()).is_err());
        Ok(())
    }
}

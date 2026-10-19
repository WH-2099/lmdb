use anyhow::{Context, Result, ensure};
use reqwest::{Url, header::HeaderMap};
use serde_json::Value;

use super::{PageSource, RepositoryRevision, is_official_url};
use crate::{http::Fetcher, model::Snapshot};

pub(super) fn validate(source: &PageSource) -> Result<()> {
    let Some(repository) = &source.repository else {
        return Ok(());
    };
    ensure!(valid_repo(&repository.repo), "invalid document repository");
    ensure!(
        safe_path(&repository.path),
        "invalid document repository path"
    );
    let revision = match &repository.revision {
        RepositoryRevision::Tracked { reference } => {
            ensure!(
                safe_path(reference),
                "invalid document repository reference"
            );
            reference
        }
        RepositoryRevision::Pinned { sha } => {
            ensure!(valid_sha(sha), "invalid pinned document repository SHA");
            sha
        }
    };
    let expected = raw_url(&repository.repo, &repository.path, revision);
    ensure!(
        source.url == expected,
        "document URL does not match its repository descriptor"
    );
    ensure!(
        is_official_url(&source.vendor, &expected),
        "document repository is not official for {}",
        source.vendor
    );
    Ok(())
}

pub(super) async fn fetch_target(
    source: &PageSource,
    fetcher: &Fetcher,
) -> Result<(String, Option<String>)> {
    validate(source)?;
    let Some(repository) = &source.repository else {
        return Ok((source.url.clone(), None));
    };
    let sha = match &repository.revision {
        RepositoryRevision::Tracked { reference } => {
            let url = revision_url(&repository.repo, reference);
            let (_, value) = fetcher
                .get_json(
                    &format!("official-{}-revision", source.vendor),
                    &url,
                    HeaderMap::new(),
                )
                .await?;
            response_sha(&repository.repo, &value)?.to_owned()
        }
        RepositoryRevision::Pinned { sha } => sha.clone(),
    };
    Ok((raw_url(&repository.repo, &repository.path, &sha), Some(sha)))
}

pub(super) fn validate_snapshot(source: &PageSource, snapshot: &Snapshot) -> Result<()> {
    validate(source)?;
    let Some(repository) = &source.repository else {
        return Ok(());
    };
    let sha = snapshot
        .upstream_revision
        .as_deref()
        .context("repository document snapshot has no upstream revision")?;
    ensure!(valid_sha(sha), "invalid document snapshot repository SHA");
    ensure!(
        snapshot.url == raw_url(&repository.repo, &repository.path, sha),
        "document snapshot URL does not match its repository and SHA"
    );
    if let RepositoryRevision::Pinned { sha: pinned } = &repository.revision {
        ensure!(sha == pinned, "document snapshot differs from pinned SHA");
    }
    Ok(())
}

pub(super) fn same_document_url(actual: &str, expected: &str) -> bool {
    actual == expected
        || hf_raw_document(actual)
            .zip(hf_raw_document(expected))
            .is_some_and(|(actual, expected)| actual == expected)
}

fn valid_repo(repo: &str) -> bool {
    let parts: Vec<_> = repo.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 96
                && safe_component(part)
                && !part.starts_with(['.', '-'])
                && !part.ends_with(['.', '-'])
                && !part.contains("..")
                && !part.contains("--")
                && !part.ends_with(".git")
        })
}

fn safe_component(component: &str) -> bool {
    !component.is_empty()
        && component != "."
        && component != ".."
        && component
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn safe_path(path: &str) -> bool {
    path.split('/').all(safe_component)
}

fn valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hub_url<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
    let mut url = Url::parse("https://huggingface.co").expect("valid Hugging Face origin");
    url.path_segments_mut()
        .expect("Hugging Face URL can have path segments")
        .extend(segments);
    url.into()
}

fn raw_url(repo: &str, path: &str, revision: &str) -> String {
    hub_url(
        repo.split('/')
            .chain(["raw", revision])
            .chain(path.split('/')),
    )
}

fn revision_url(repo: &str, reference: &str) -> String {
    hub_url(
        ["api", "models"]
            .into_iter()
            .chain(repo.split('/'))
            .chain(["revision", reference]),
    )
}

fn response_sha<'a>(repo: &str, value: &'a Value) -> Result<&'a str> {
    if let Some(id) = value.get("id") {
        ensure!(
            id.as_str() == Some(repo),
            "repository revision response does not identify {repo}"
        );
    }
    let sha = value
        .get("sha")
        .and_then(Value::as_str)
        .context("repository revision response has no SHA")?;
    ensure!(
        valid_sha(sha),
        "repository revision response has invalid SHA"
    );
    Ok(sha)
}

fn hf_raw_document(raw: &str) -> Option<(String, String)> {
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("huggingface.co")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let parts: Vec<_> = url.path_segments()?.collect();
    if parts.len() < 5 || parts[2] != "raw" {
        return None;
    }
    let repo = format!("{}/{}", parts[0], parts[1]);
    let path = parts[4..].join("/");
    let revision = parts[3].replace("%2F", "/");
    (valid_repo(&repo)
        && safe_path(&path)
        && safe_path(&revision)
        && raw_url(&repo, &path, &revision) == raw)
        .then_some((repo, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{DocumentKind, Format, RepositorySource};
    use serde_json::json;

    const OLD_SHA: &str = "1111111111111111111111111111111111111111";
    const NEW_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";

    fn source(revision: RepositoryRevision) -> PageSource {
        let reference = match &revision {
            RepositoryRevision::Tracked { reference } => reference,
            RepositoryRevision::Pinned { sha } => sha,
        };
        PageSource {
            id: "qwen-card".into(),
            vendor: "qwen".into(),
            kind: DocumentKind::ModelCard,
            url: raw_url("Qwen/Qwen3-8B", "README.md", reference),
            format: Format::Markdown,
            records: vec![],
            repository: Some(RepositorySource {
                repo: "Qwen/Qwen3-8B".into(),
                path: "README.md".into(),
                revision,
            }),
        }
    }

    fn tracked() -> PageSource {
        source(RepositoryRevision::Tracked {
            reference: "main".into(),
        })
    }

    fn snapshot(sha: &str) -> Snapshot {
        Snapshot {
            schema_version: 1,
            id: "snapshot".into(),
            source: "official-qwen".into(),
            url: raw_url("Qwen/Qwen3-8B", "README.md", sha),
            retrieved_at: "2026-10-19T00:00:00Z".into(),
            sha256: "0".repeat(64),
            upstream_revision: Some(sha.into()),
        }
    }

    #[test]
    fn descriptors_reject_other_repositories_and_unsafe_paths() {
        assert!(validate(&tracked()).is_ok());
        for repo in ["Qwen", "Qwen/a/b", "Qwen/..", "Qwen/evil?x", "other/model"] {
            let mut source = tracked();
            source.repository.as_mut().unwrap().repo = repo.into();
            source.url = raw_url(repo, "README.md", "main");
            assert!(validate(&source).is_err(), "{repo}");
        }
        for path in [
            "",
            "../README.md",
            "a/./README.md",
            "/README.md",
            "a//README.md",
            "README.md?x",
            "a\\README.md",
            "%2e%2e/README.md",
        ] {
            let mut source = tracked();
            source.repository.as_mut().unwrap().path = path.into();
            source.url = raw_url("Qwen/Qwen3-8B", path, "main");
            assert!(validate(&source).is_err(), "{path}");
        }
        for reference in ["", "../main", "main?x", "main#x", "%2e%2e"] {
            assert!(
                validate(&source(RepositoryRevision::Tracked {
                    reference: reference.into(),
                }))
                .is_err(),
                "{reference}"
            );
        }
        let mut source = tracked();
        source.url = raw_url("Qwen/Qwen3-8B", "config.json", "main");
        assert!(validate(&source).is_err());
        assert!(
            validate(&self::source(RepositoryRevision::Pinned {
                sha: "short".into()
            }))
            .is_err()
        );
    }

    #[test]
    fn revision_responses_require_a_complete_sha_and_matching_identity() {
        assert_eq!(
            response_sha(
                "Qwen/Qwen3-8B",
                &json!({"id": "Qwen/Qwen3-8B", "sha": NEW_SHA})
            )
            .unwrap(),
            NEW_SHA
        );
        assert_eq!(
            response_sha("Qwen/Qwen3-8B", &json!({"sha": OLD_SHA})).unwrap(),
            OLD_SHA
        );
        for value in [
            json!({}),
            json!({"sha": null}),
            json!({"sha": "123"}),
            json!({"sha": "g".repeat(40)}),
            json!({"id": "Qwen/other", "sha": NEW_SHA}),
            json!({"id": null, "sha": NEW_SHA}),
        ] {
            assert!(response_sha("Qwen/Qwen3-8B", &value).is_err(), "{value}");
        }
    }

    #[test]
    fn references_are_encoded_as_one_url_segment() {
        assert_eq!(
            revision_url("Qwen/Qwen3-8B", "refs/pr/1"),
            "https://huggingface.co/api/models/Qwen/Qwen3-8B/revision/refs%2Fpr%2F1"
        );
        let source = source(RepositoryRevision::Tracked {
            reference: "refs/pr/1".into(),
        });
        assert_eq!(
            source.url,
            "https://huggingface.co/Qwen/Qwen3-8B/raw/refs%2Fpr%2F1/README.md"
        );
        assert!(validate(&source).is_ok());
        assert!(same_document_url(&source.url, &snapshot(NEW_SHA).url));
    }

    #[test]
    fn tracked_snapshots_preserve_historical_commits_and_pins_require_their_sha() {
        let source = tracked();
        assert!(validate_snapshot(&source, &snapshot(OLD_SHA)).is_ok());
        assert!(validate_snapshot(&source, &snapshot(NEW_SHA)).is_ok());
        let pinned = self::source(RepositoryRevision::Pinned {
            sha: OLD_SHA.into(),
        });
        assert!(validate(&pinned).is_ok());
        assert!(validate_snapshot(&pinned, &snapshot(OLD_SHA)).is_ok());
        assert!(validate_snapshot(&pinned, &snapshot(NEW_SHA)).is_err());
        let mut missing = snapshot(OLD_SHA);
        missing.upstream_revision = None;
        assert!(validate_snapshot(&source, &missing).is_err());
        let mut mismatch = snapshot(OLD_SHA);
        mismatch.url = snapshot(NEW_SHA).url;
        assert!(validate_snapshot(&source, &mismatch).is_err());
    }

    #[test]
    fn parser_matching_only_relaxes_the_hugging_face_revision() {
        let expected = snapshot(OLD_SHA).url;
        assert!(same_document_url(&snapshot(NEW_SHA).url, &expected));
        assert!(same_document_url(&tracked().url, &expected));
        for actual in [
            expected.replace("README.md", "config.json"),
            expected.replace("Qwen3-8B", "Qwen3-4B"),
            expected.replace("Qwen/", "other/"),
            expected.replace("https://", "http://"),
            expected.replace("huggingface.co", "example.com"),
            expected.replace("/raw/", "/blob/"),
            format!("{expected}?download=true"),
            format!("{expected}#fragment"),
            expected.replace(OLD_SHA, "%2e%2e"),
        ] {
            assert!(!same_document_url(&actual, &expected), "{actual}");
        }
        assert!(!same_document_url(
            "https://example.com/a",
            "https://example.com/b"
        ));
        assert!(same_document_url(
            "https://example.com/a",
            "https://example.com/a"
        ));
    }
}

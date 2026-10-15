use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use reqwest::{
    Client, StatusCode, Url,
    header::{ETAG, HeaderMap, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, RETRY_AFTER},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::{
    OffsetDateTime,
    format_description::well_known::{Rfc2822, Rfc3339},
};

use crate::{
    model::{SCHEMA_VERSION, Snapshot},
    store::write_bytes,
};

const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_RETRIES: u32 = 3;
const RETRY_BUDGET: Duration = Duration::from_secs(30);

pub struct Fetcher {
    client: Client,
    cache_root: PathBuf,
}

#[derive(Deserialize, Serialize)]
struct Receipt {
    snapshot: Snapshot,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl Fetcher {
    pub fn new(cache_root: &Path) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(30))
                .user_agent(concat!("model-metadata/", env!("CARGO_PKG_VERSION")))
                .build()
                .context("building HTTP client")?,
            cache_root: cache_root.into(),
        })
    }

    pub async fn get_json(
        &self,
        source: &str,
        url: &str,
        headers: HeaderMap,
    ) -> Result<(Snapshot, Value)> {
        let parsed = Url::parse(url).context("invalid request URL")?;
        ensure!(
            matches!(parsed.scheme(), "http" | "https") && parsed.has_host(),
            "invalid HTTP URL"
        );
        ensure!(
            parsed.username().is_empty() && parsed.password().is_none(),
            "credentials must use headers"
        );
        let public = headers.is_empty();
        let receipt_path = self
            .cache_root
            .join("http")
            .join(format!("{}.json", sha256(url.as_bytes())));
        let receipt: Option<Receipt> = if public {
            match fs::read(&receipt_path) {
                Ok(bytes) => {
                    let receipt: Receipt = serde_json::from_slice(&bytes)
                        .with_context(|| format!("invalid HTTP cache receipt for {url}"))?;
                    let snapshot = &receipt.snapshot;
                    ensure!(
                        snapshot.schema_version == SCHEMA_VERSION
                            && snapshot.url == url
                            && snapshot.source == source
                            && snapshot.sha256.len() == 64
                            && snapshot
                                .sha256
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                            && snapshot.id == format!("{source}:{}", snapshot.sha256),
                        "HTTP cache receipt does not match {url}"
                    );
                    Some(receipt)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(error).with_context(|| format!("reading HTTP cache for {url}"));
                }
            }
        } else {
            None
        };
        let mut remaining = RETRY_BUDGET;
        for retry in 0..=MAX_RETRIES {
            let mut request = self.client.get(parsed.clone()).headers(headers.clone());
            if let Some(receipt) = &receipt {
                if let Some(etag) = &receipt.etag {
                    request = request.header(IF_NONE_MATCH, etag);
                }
                if let Some(modified) = &receipt.last_modified {
                    request = request.header(IF_MODIFIED_SINCE, modified);
                }
            }
            let response = request
                .send()
                .await
                .with_context(|| format!("request failed for {url}"))?;
            let status = response.status();
            if status == StatusCode::NOT_MODIFIED {
                let receipt =
                    receipt.context("unexpected HTTP 304 without a public cache receipt")?;
                let path = self
                    .cache_root
                    .join("snapshots")
                    .join(format!("{}.json", receipt.snapshot.sha256));
                let bytes = fs::read(&path)
                    .with_context(|| format!("missing or unreadable cached snapshot for {url}"))?;
                ensure!(
                    sha256(&bytes) == receipt.snapshot.sha256,
                    "cached snapshot SHA-256 mismatch for {url}"
                );
                let value = serde_json::from_slice(&bytes)
                    .with_context(|| format!("invalid cached JSON for {url}"))?;
                return Ok((receipt.snapshot, value));
            }
            if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                ensure!(
                    retry < MAX_RETRIES,
                    "HTTP {status} for {url}; retry limit reached"
                );
                let delay = retry_delay(
                    response.headers(),
                    retry,
                    OffsetDateTime::now_utc(),
                    remaining,
                )
                .with_context(|| format!("HTTP {status} for {url}"))?;
                remaining -= delay;
                drop(response);
                tokio::time::sleep(delay).await;
                continue;
            }
            ensure!(status.is_success(), "HTTP {status} for {url}");
            ensure!(
                response
                    .content_length()
                    .is_none_or(|length| length <= MAX_BODY_BYTES as u64),
                "response exceeds 64 MiB for {url}"
            );
            let etag = header_text(response.headers(), ETAG);
            let last_modified = header_text(response.headers(), LAST_MODIFIED);
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.with_context(|| format!("reading response from {url}"))?;
                ensure!(
                    chunk.len() <= MAX_BODY_BYTES - bytes.len(),
                    "response exceeds 64 MiB for {url}"
                );
                bytes.extend_from_slice(&chunk);
            }
            let value = serde_json::from_slice(&bytes)
                .with_context(|| format!("invalid JSON from {url}"))?;
            let digest = sha256(&bytes);
            let snapshot = Snapshot {
                schema_version: SCHEMA_VERSION,
                id: format!("{source}:{digest}"),
                source: source.into(),
                url: url.into(),
                retrieved_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
                sha256: digest.clone(),
                upstream_revision: None,
            };
            write_bytes(
                &self
                    .cache_root
                    .join("snapshots")
                    .join(format!("{digest}.json")),
                &bytes,
            )?;
            if public {
                write_bytes(
                    &receipt_path,
                    &serde_json::to_vec(&Receipt {
                        snapshot: snapshot.clone(),
                        etag,
                        last_modified,
                    })?,
                )?;
            }
            return Ok((snapshot, value));
        }
        unreachable!("the final failed attempt returns an error")
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn header_text(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn retry_delay(
    headers: &HeaderMap,
    retry: u32,
    now: OffsetDateTime,
    remaining: Duration,
) -> Result<Duration> {
    let delay = match headers.get(RETRY_AFTER) {
        None => Duration::from_secs(1 << retry),
        Some(value) => {
            let value = value.to_str().context("invalid Retry-After header")?;
            match value.parse::<u64>() {
                Ok(seconds) => Duration::from_secs(seconds),
                Err(_) => {
                    let at = OffsetDateTime::parse(value, &Rfc2822)
                        .context("invalid Retry-After date")?;
                    Duration::try_from(at - now).unwrap_or_default()
                }
            }
        }
    };
    ensure!(
        delay <= remaining,
        "Retry-After exceeds the remaining 30-second retry budget"
    );
    Ok(delay)
}

pub fn checked_next_url(current: &str, next: &str) -> Result<String> {
    let current = Url::parse(current).context("invalid current page URL")?;
    let next = current.join(next).context("invalid next page URL")?;
    ensure!(
        matches!(current.scheme(), "http" | "https") && current.has_host(),
        "invalid current HTTP URL"
    );
    ensure!(
        next.username().is_empty() && next.password().is_none(),
        "pagination URL contains credentials"
    );
    ensure!(
        next.scheme() == current.scheme()
            && next.host_str() == current.host_str()
            && next.port_or_known_default() == current.port_or_known_default(),
        "pagination URL changes origin"
    );
    ensure!(next != current, "pagination URL did not advance");
    if next.fragment().is_some() {
        bail!("pagination URL contains a fragment");
    }
    Ok(next.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn server(responses: Vec<String>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            responses
                .into_iter()
                .map(|response| {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut byte = [0];
                    while !request.ends_with(b"\r\n\r\n") {
                        socket.read_exact(&mut byte).unwrap();
                        request.push(byte[0]);
                    }
                    socket.write_all(response.as_bytes()).unwrap();
                    String::from_utf8(request).unwrap()
                })
                .collect()
        });
        (url, worker)
    }

    fn response(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn conditional_cache_checks_integrity_and_invalid_responses_preserve_it() {
        let body = r#"{"models":["a"]}"#;
        let (url, worker) = server(vec![
            response(
                "200 OK",
                "ETag: \"v1\"\r\nLast-Modified: Thu, 15 Oct 2026 01:00:00 GMT\r\n",
                body,
            ),
            response("304 Not Modified", "", ""),
            response("200 OK", "", "invalid JSON"),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_BODY_BYTES + 1
            ),
            response("304 Not Modified", "", ""),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let first = fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        let receipt_path = cache
            .path()
            .join("http")
            .join(format!("{}.json", sha256(url.as_bytes())));
        let receipt = fs::read(&receipt_path).unwrap();
        assert_eq!(
            fetcher
                .get_json("fixture", &url, HeaderMap::new())
                .await
                .unwrap(),
            first
        );
        assert!(
            fetcher
                .get_json("fixture", &url, HeaderMap::new())
                .await
                .unwrap_err()
                .to_string()
                .contains("invalid JSON")
        );
        assert!(
            fetcher
                .get_json("fixture", &url, HeaderMap::new())
                .await
                .unwrap_err()
                .to_string()
                .contains("exceeds 64 MiB")
        );
        assert_eq!(fs::read(&receipt_path).unwrap(), receipt);
        let raw_path = cache
            .path()
            .join("snapshots")
            .join(format!("{}.json", first.0.sha256));
        assert_eq!(fs::read(&raw_path).unwrap(), body.as_bytes());
        fs::write(raw_path, b"{}").unwrap();
        assert!(
            fetcher
                .get_json("fixture", &url, HeaderMap::new())
                .await
                .unwrap_err()
                .to_string()
                .contains("SHA-256 mismatch")
        );
        let requests = worker.join().unwrap();
        assert!(!requests[0].to_ascii_lowercase().contains("if-none-match"));
        assert!(
            requests[1]
                .to_ascii_lowercase()
                .contains("if-none-match: \"v1\"")
        );
        assert!(
            requests[1]
                .to_ascii_lowercase()
                .contains("if-modified-since:")
        );
    }

    #[tokio::test]
    async fn authenticated_requests_do_not_reuse_or_replace_public_receipts() {
        let (url, worker) = server(vec![
            response("200 OK", "ETag: public\r\n", "{}"),
            response("200 OK", "ETag: private\r\n", r#"{"private":true}"#),
            response("304 Not Modified", "", ""),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let first = fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer local-fixture".parse().unwrap());
        fetcher.get_json("fixture", &url, headers).await.unwrap();
        assert_eq!(
            fetcher
                .get_json("fixture", &url, HeaderMap::new())
                .await
                .unwrap(),
            first
        );
        let requests = worker.join().unwrap();
        assert!(!requests[1].to_ascii_lowercase().contains("if-none-match"));
        assert!(
            requests[2]
                .to_ascii_lowercase()
                .contains("if-none-match: public")
        );
    }

    #[tokio::test]
    async fn retries_are_bounded_and_redirects_are_rejected() {
        let (url, worker) = server(vec![
            response("429 Too Many Requests", "Retry-After: 0\r\n", ""),
            response("503 Service Unavailable", "Retry-After: 0\r\n", ""),
            response("500 Internal Server Error", "Retry-After: 0\r\n", ""),
            response("503 Service Unavailable", "Retry-After: 0\r\n", ""),
            response("429 Too Many Requests", "Retry-After: 31\r\n", ""),
            response("302 Found", "Location: /other\r\n", ""),
            response("401 Unauthorized", "", ""),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        assert!(
            fetcher
                .get_json("fixture", &url, HeaderMap::new())
                .await
                .unwrap_err()
                .to_string()
                .contains("retry limit")
        );
        let error = fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("retry budget"));
        assert!(
            fetcher
                .get_json("fixture", &url, HeaderMap::new())
                .await
                .unwrap_err()
                .to_string()
                .contains("302")
        );
        assert!(
            fetcher
                .get_json("fixture", &url, HeaderMap::new())
                .await
                .unwrap_err()
                .to_string()
                .contains("401")
        );
        assert_eq!(worker.join().unwrap().len(), 7);
        assert!(!cache.path().join("http").exists());
    }

    #[test]
    fn retry_after_respects_dates_and_the_remaining_budget() {
        let now = OffsetDateTime::parse("2026-10-15T01:00:00Z", &Rfc3339).unwrap();
        for (value, expected) in [
            ("15", 15),
            ("Thu, 15 Oct 2026 01:00:12 GMT", 12),
            ("Thu, 15 Oct 2026 00:00:00 GMT", 0),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(RETRY_AFTER, value.parse().unwrap());
            assert_eq!(
                retry_delay(&headers, 0, now, RETRY_BUDGET).unwrap(),
                Duration::from_secs(expected)
            );
        }
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, "31".parse().unwrap());
        assert!(retry_delay(&headers, 0, now, RETRY_BUDGET).is_err());
        headers.insert(RETRY_AFTER, "15".parse().unwrap());
        assert!(retry_delay(&headers, 0, now, Duration::from_secs(14)).is_err());
        headers.insert(RETRY_AFTER, "garbage".parse().unwrap());
        assert!(retry_delay(&headers, 0, now, RETRY_BUDGET).is_err());
    }

    #[test]
    fn pagination_stays_on_the_same_origin() {
        let current = "https://example.test/models?page=1";
        assert_eq!(
            checked_next_url(current, "?page=2").unwrap(),
            "https://example.test/models?page=2"
        );
        assert_eq!(
            checked_next_url(current, "https://example.test:443/models?page=2").unwrap(),
            "https://example.test/models?page=2"
        );
        for next in [
            "https://other.test/models",
            "http://example.test/models",
            "https://example.test:444/models",
            "https://user:secret@example.test/models",
            "//other.test/models",
            "?page=1",
            "?page=2#fragment",
        ] {
            assert!(checked_next_url(current, next).is_err(), "accepted {next}");
        }
    }
}

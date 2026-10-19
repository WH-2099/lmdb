use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use reqwest::{
    Client, StatusCode, Url,
    header::{
        CONTENT_TYPE, ETAG, HeaderMap, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LINK,
        LOCATION, RETRY_AFTER,
    },
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
const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_RETRIES: u32 = 3;
const RETRY_BUDGET: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REDIRECTS: usize = 5;

pub struct Fetcher {
    client: Client,
    cache_root: PathBuf,
    total_timeout: Duration,
}

#[derive(Clone, Copy)]
enum ResponseKind<'a> {
    Json,
    Document(Option<&'a (dyn Fn(&str) -> bool + Sync)>),
}

#[derive(Deserialize, Serialize)]
struct Receipt {
    snapshot: Snapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    requested_url: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
    #[serde(default)]
    link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
}

impl Fetcher {
    pub fn new(cache_root: &Path) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .timeout(REQUEST_TIMEOUT)
                .user_agent(concat!("model-metadata/", env!("CARGO_PKG_VERSION")))
                .build()
                .context("building HTTP client")?,
            cache_root: cache_root.into(),
            total_timeout: TOTAL_TIMEOUT,
        })
    }

    pub async fn get_json(
        &self,
        source: &str,
        url: &str,
        headers: HeaderMap,
    ) -> Result<(Snapshot, Value)> {
        let (snapshot, value, _) = self.get_json_with_link(source, url, headers).await?;
        Ok((snapshot, value))
    }

    /// Return the next-page URL from the HTTP Link header, including cached responses.
    pub async fn get_json_with_link(
        &self,
        source: &str,
        url: &str,
        headers: HeaderMap,
    ) -> Result<(Snapshot, Value, Option<String>)> {
        self.get(source, url, headers, ResponseKind::Json, |bytes| {
            Ok(serde_json::from_slice(bytes)?)
        })
        .await
    }

    /// Fetch an anonymous document without redirects, preserving bytes and Content-Type.
    pub async fn get_document(
        &self,
        source: &str,
        url: &str,
    ) -> Result<(Snapshot, Vec<u8>, Option<String>)> {
        self.get(
            source,
            url,
            HeaderMap::new(),
            ResponseKind::Document(None),
            document_bytes,
        )
        .await
    }

    /// Follow only explicit, anonymous document redirects approved at every HTTPS hop.
    pub async fn get_document_with_redirects(
        &self,
        source: &str,
        url: &str,
        allowed: impl Fn(&str) -> bool + Sync,
    ) -> Result<(Snapshot, Vec<u8>, Option<String>)> {
        self.get(
            source,
            url,
            HeaderMap::new(),
            ResponseKind::Document(Some(&allowed)),
            document_bytes,
        )
        .await
    }

    async fn get<T>(
        &self,
        source: &str,
        url: &str,
        headers: HeaderMap,
        kind: ResponseKind<'_>,
        parse: impl Fn(&[u8]) -> Result<T>,
    ) -> Result<(Snapshot, T, Option<String>)> {
        tokio::time::timeout(
            self.total_timeout,
            self.get_inner(source, url, headers, kind, parse),
        )
        .await
        .with_context(|| format!("request deferred for {url}; total HTTP deadline exceeded"))?
    }

    async fn get_inner<T>(
        &self,
        source: &str,
        url: &str,
        headers: HeaderMap,
        kind: ResponseKind<'_>,
        parse: impl Fn(&[u8]) -> Result<T>,
    ) -> Result<(Snapshot, T, Option<String>)> {
        let started = tokio::time::Instant::now();
        let parsed = Url::parse(url).context("invalid request URL")?;
        if let ResponseKind::Document(Some(allowed)) = kind {
            checked_document_url(&parsed, allowed)?;
        }
        ensure!(
            matches!(parsed.scheme(), "http" | "https") && parsed.has_host(),
            "invalid HTTP URL"
        );
        ensure!(
            parsed.username().is_empty() && parsed.password().is_none(),
            "credentials must use headers"
        );
        let (receipt_dir, body_dir, extension, max_bytes, body_name) = match kind {
            ResponseKind::Json => ("http", "snapshots", "json", MAX_BODY_BYTES, "JSON"),
            ResponseKind::Document(_) => (
                "document-http",
                "documents",
                "bin",
                MAX_DOCUMENT_BYTES,
                "document",
            ),
        };
        let public = headers.is_empty();
        let receipt_path = self
            .cache_root
            .join(receipt_dir)
            .join(format!("{}.json", sha256(url.as_bytes())));
        let receipt: Option<Receipt> = if public {
            match fs::read(&receipt_path) {
                Ok(bytes) => {
                    let receipt: Receipt = serde_json::from_slice(&bytes)
                        .with_context(|| format!("invalid HTTP cache receipt for {url}"))?;
                    let snapshot = &receipt.snapshot;
                    ensure!(
                        snapshot.schema_version == SCHEMA_VERSION
                            && receipt.requested_url.as_deref().unwrap_or(&snapshot.url) == url
                            && snapshot.source == source
                            && snapshot.sha256.len() == 64
                            && snapshot
                                .sha256
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                            && snapshot.id == format!("{source}:{}", snapshot.sha256),
                        "HTTP cache receipt does not match {url}"
                    );
                    if snapshot.url != url {
                        let ResponseKind::Document(Some(allowed)) = kind else {
                            bail!("HTTP cache receipt contains an unexpected redirect for {url}");
                        };
                        checked_document_url(&Url::parse(&snapshot.url)?, allowed)?;
                    }
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
        'attempt: for retry in 0..=MAX_RETRIES {
            let mut current = parsed.clone();
            let mut visited = vec![current.clone()];
            let response = loop {
                let mut request = self.client.get(current.clone()).headers(headers.clone());
                // Validators belong to the final representation, never an intermediate redirect.
                if let Some(receipt) = receipt.as_ref().filter(|r| {
                    r.snapshot.url == current.as_str()
                        || (visited.len() == 1 && r.snapshot.url == url)
                }) {
                    if let Some(etag) = &receipt.etag {
                        request = request.header(IF_NONE_MATCH, etag);
                    }
                    if let Some(modified) = &receipt.last_modified {
                        request = request.header(IF_MODIFIED_SINCE, modified);
                    }
                }
                let response = match request.send().await {
                    Ok(response) => response,
                    Err(error) if transient(&error) => {
                        self.wait_retry(&HeaderMap::new(), retry, &mut remaining, started)
                            .await
                            .with_context(|| format!("request failed for {url}: {error}"))?;
                        continue 'attempt;
                    }
                    Err(error) => {
                        return Err(error).with_context(|| format!("request failed for {url}"));
                    }
                };
                if let ResponseKind::Document(Some(allowed)) = kind
                    && matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308)
                {
                    ensure!(
                        visited.len() <= MAX_REDIRECTS,
                        "document redirect limit reached for {url}"
                    );
                    let location = response
                        .headers()
                        .get(LOCATION)
                        .context("document redirect has no Location")?
                        .to_str()
                        .context("invalid document redirect Location")?;
                    let next = current
                        .join(location)
                        .context("invalid document redirect URL")?;
                    checked_document_url(&next, allowed)?;
                    ensure!(!visited.contains(&next), "document redirect loop for {url}");
                    visited.push(next.clone());
                    current = next;
                    continue;
                }
                break response;
            };
            let final_url = if visited.len() == 1 {
                url
            } else {
                current.as_str()
            };
            let status = response.status();
            if status == StatusCode::NOT_MODIFIED {
                let mut receipt =
                    receipt.context("unexpected HTTP 304 without a public cache receipt")?;
                ensure!(
                    receipt.snapshot.url == final_url,
                    "HTTP 304 URL does not match cached representation"
                );
                let path = self
                    .cache_root
                    .join(body_dir)
                    .join(format!("{}.{extension}", receipt.snapshot.sha256));
                let bytes = fs::read(&path)
                    .with_context(|| format!("missing or unreadable cached snapshot for {url}"))?;
                ensure!(
                    sha256(&bytes) == receipt.snapshot.sha256,
                    "cached snapshot SHA-256 mismatch for {url}"
                );
                let value = parse(&bytes)
                    .with_context(|| format!("invalid cached {body_name} for {url}"))?;
                match kind {
                    ResponseKind::Json => {
                        receipt.link = link_header(response.headers())?.or(receipt.link);
                    }
                    ResponseKind::Document(_) => {
                        receipt.content_type =
                            document_content_type(response.headers())?.or(receipt.content_type);
                    }
                }
                let metadata = response_metadata(kind, &receipt.link, &receipt.content_type)?;
                receipt.etag = header_text(response.headers(), ETAG).or(receipt.etag);
                receipt.last_modified =
                    header_text(response.headers(), LAST_MODIFIED).or(receipt.last_modified);
                write_bytes(&receipt_path, &serde_json::to_vec(&receipt)?)?;
                return Ok((receipt.snapshot, value, metadata));
            }
            if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                let retry_headers = response.headers().clone();
                drop(response);
                self.wait_retry(&retry_headers, retry, &mut remaining, started)
                    .await
                    .with_context(|| format!("HTTP {status} for {url}"))?;
                continue;
            }
            ensure!(status.is_success(), "HTTP {status} for {url}");
            ensure!(
                response
                    .content_length()
                    .is_none_or(|length| length <= max_bytes as u64),
                "response exceeds {} MiB for {url}",
                max_bytes / (1024 * 1024)
            );
            let etag = header_text(response.headers(), ETAG);
            let last_modified = header_text(response.headers(), LAST_MODIFIED);
            let (link, content_type) = match kind {
                ResponseKind::Json => (link_header(response.headers())?, None),
                ResponseKind::Document(_) => (None, document_content_type(response.headers())?),
            };
            let metadata = response_metadata(kind, &link, &content_type)?;
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) if transient(&error) => {
                        drop(stream);
                        self.wait_retry(&HeaderMap::new(), retry, &mut remaining, started)
                            .await
                            .with_context(|| format!("reading response from {url}: {error}"))?;
                        continue 'attempt;
                    }
                    Err(error) => {
                        return Err(error).with_context(|| format!("reading response from {url}"));
                    }
                };
                ensure!(
                    chunk.len() <= max_bytes - bytes.len(),
                    "response exceeds {} MiB for {url}",
                    max_bytes / (1024 * 1024)
                );
                bytes.extend_from_slice(&chunk);
            }
            let value = parse(&bytes).with_context(|| format!("invalid {body_name} from {url}"))?;
            let digest = sha256(&bytes);
            let snapshot = Snapshot {
                schema_version: SCHEMA_VERSION,
                id: format!("{source}:{digest}"),
                source: source.into(),
                url: final_url.into(),
                retrieved_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
                sha256: digest.clone(),
                upstream_revision: None,
            };
            write_bytes(
                &self
                    .cache_root
                    .join(body_dir)
                    .join(format!("{digest}.{extension}")),
                &bytes,
            )?;
            if public {
                write_bytes(
                    &receipt_path,
                    &serde_json::to_vec(&Receipt {
                        snapshot: snapshot.clone(),
                        requested_url: Some(url.into()),
                        etag,
                        last_modified,
                        link,
                        content_type,
                    })?,
                )?;
            }
            return Ok((snapshot, value, metadata));
        }
        unreachable!("the final failed attempt returns an error")
    }

    async fn wait_retry(
        &self,
        headers: &HeaderMap,
        retry: u32,
        remaining: &mut Duration,
        started: tokio::time::Instant,
    ) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let budget = (*remaining).min(self.total_timeout.saturating_sub(started.elapsed()));
        let delay = retry_delay(headers, retry, now, budget)?;
        ensure!(
            retry < MAX_RETRIES,
            "request deferred; retry limit reached; retry_at={}",
            retry_at(now, delay)
        );
        *remaining -= delay;
        tokio::time::sleep(delay).await;
        Ok(())
    }
}

fn document_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        !bytes.iter().all(u8::is_ascii_whitespace),
        "document is empty"
    );
    Ok(bytes.to_vec())
}

fn checked_document_url(url: &Url, allowed: &dyn Fn(&str) -> bool) -> Result<()> {
    ensure!(
        url.scheme() == "https"
            && url.has_host()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.port().is_none()
            && allowed(url.as_str()),
        "document redirect URL is not allowed: {url}"
    );
    Ok(())
}

fn transient(error: &reqwest::Error) -> bool {
    // reqwest also classifies interrupted byte streams as decode errors.
    error.is_timeout()
        || error.is_connect()
        || error.is_request()
        || error.is_body()
        || error.is_decode()
}

fn response_metadata(
    kind: ResponseKind,
    link: &Option<String>,
    content_type: &Option<String>,
) -> Result<Option<String>> {
    match kind {
        ResponseKind::Json => next_link(link.as_deref()),
        ResponseKind::Document(_) => {
            if let Some(content_type) = content_type {
                let mime = content_type.split(';').next().unwrap_or("").trim();
                ensure!(
                    [
                        "text/html",
                        "application/xhtml+xml",
                        "text/markdown",
                        "text/x-markdown",
                        "text/plain",
                        "application/pdf",
                    ]
                    .iter()
                    .any(|allowed| mime.eq_ignore_ascii_case(allowed)),
                    "unsupported document Content-Type: {content_type}"
                );
            }
            Ok(content_type.clone())
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn document_content_type(headers: &HeaderMap) -> Result<Option<String>> {
    headers
        .get(CONTENT_TYPE)
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .context("invalid document Content-Type header")
        })
        .transpose()
}

fn header_text(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn link_header(headers: &HeaderMap) -> Result<Option<String>> {
    let links = headers
        .get_all(LINK)
        .iter()
        .map(|value| value.to_str().context("invalid Link header"))
        .collect::<Result<Vec<_>>>()?;
    Ok((!links.is_empty()).then(|| links.join(", ")))
}

fn next_link(header: Option<&str>) -> Result<Option<String>> {
    let Some(header) = header else {
        return Ok(None);
    };
    let bytes = header.as_bytes();
    let mut cursor = 0;
    let mut next = None;
    while cursor < bytes.len() {
        skip_link_space(bytes, &mut cursor);
        ensure!(bytes.get(cursor) == Some(&b'<'), "invalid Link target");
        cursor += 1;
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] != b'>' {
            cursor += 1;
        }
        ensure!(
            cursor < bytes.len() && cursor > start,
            "invalid Link target"
        );
        let target = &header[start..cursor];
        ensure!(
            !target
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control()),
            "invalid whitespace in Link target"
        );
        cursor += 1;
        let mut relation = None;
        loop {
            skip_link_space(bytes, &mut cursor);
            match bytes.get(cursor) {
                None | Some(b',') => break,
                Some(b';') => cursor += 1,
                _ => bail!("invalid Link parameter separator"),
            }
            skip_link_space(bytes, &mut cursor);
            let start = cursor;
            while bytes.get(cursor).is_some_and(|byte| {
                byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(byte)
            }) {
                cursor += 1;
            }
            ensure!(cursor > start, "missing Link parameter name");
            let name = &header[start..cursor];
            skip_link_space(bytes, &mut cursor);
            let value = if bytes.get(cursor) == Some(&b'=') {
                cursor += 1;
                skip_link_space(bytes, &mut cursor);
                Some(link_parameter(bytes, &mut cursor)?)
            } else {
                None
            };
            if name.eq_ignore_ascii_case("rel") {
                ensure!(relation.is_none(), "duplicate Link rel parameter");
                relation = Some(value.context("Link rel has no value")?);
            }
        }
        if relation.is_some_and(|value| value.split_ascii_whitespace().any(|rel| rel == "next")) {
            ensure!(next.is_none(), "multiple next links");
            next = Some(target.to_owned());
        }
        if cursor < bytes.len() {
            cursor += 1;
            skip_link_space(bytes, &mut cursor);
            ensure!(cursor < bytes.len(), "trailing comma in Link header");
        }
    }
    Ok(next)
}

fn skip_link_space(bytes: &[u8], cursor: &mut usize) {
    while bytes
        .get(*cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        *cursor += 1;
    }
}

fn link_parameter(bytes: &[u8], cursor: &mut usize) -> Result<String> {
    let mut value = Vec::new();
    if bytes.get(*cursor) == Some(&b'"') {
        *cursor += 1;
        loop {
            let byte = *bytes
                .get(*cursor)
                .context("unterminated Link quoted value")?;
            *cursor += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let escaped = *bytes.get(*cursor).context("unterminated Link escape")?;
                    ensure!(!escaped.is_ascii_control(), "invalid Link escape");
                    value.push(escaped);
                    *cursor += 1;
                }
                byte if !byte.is_ascii_control() => value.push(byte),
                _ => bail!("invalid control character in Link value"),
            }
        }
    } else {
        while bytes
            .get(*cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(byte))
        {
            value.push(bytes[*cursor]);
            *cursor += 1;
        }
        ensure!(!value.is_empty(), "missing Link parameter value");
    }
    String::from_utf8(value).context("Link parameter is not UTF-8")
}

fn retry_delay(
    headers: &HeaderMap,
    retry: u32,
    now: OffsetDateTime,
    remaining: Duration,
) -> Result<Duration> {
    let delay = match headers.get(RETRY_AFTER) {
        None => rate_limit_delay(headers)?.unwrap_or_else(|| Duration::from_secs(1 << retry)),
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
        "request deferred; delay exceeds remaining retry budget or total deadline; retry_at={}",
        retry_at(now, delay)
    );
    Ok(delay)
}

fn retry_at(now: OffsetDateTime, delay: Duration) -> String {
    time::Duration::try_from(delay)
        .ok()
        .and_then(|delay| now.checked_add(delay))
        .and_then(|at| at.format(&Rfc3339).ok())
        .unwrap_or_else(|| "out_of_range".into())
}

fn rate_limit_delay(headers: &HeaderMap) -> Result<Option<Duration>> {
    let mut delay: Option<Duration> = None;
    for value in headers.get_all("ratelimit") {
        let value = value.to_str().context("invalid RateLimit header")?;
        for quota in value.split(',') {
            let mut remaining = None;
            let mut reset = None;
            for parameter in quota.split(';').skip(1) {
                let Some((name, value)) = parameter.trim().split_once('=') else {
                    continue;
                };
                let field = match name.trim() {
                    "r" => &mut remaining,
                    "t" => &mut reset,
                    _ => continue,
                };
                ensure!(field.is_none(), "duplicate RateLimit parameter");
                *field = Some(
                    value
                        .trim()
                        .parse::<u64>()
                        .context("invalid RateLimit parameter")?,
                );
            }
            if remaining == Some(0) {
                let reset =
                    Duration::from_secs(reset.context("exhausted RateLimit has no reset time")?);
                delay = Some(delay.unwrap_or_default().max(reset));
            }
        }
    }
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

    fn server(
        responses: Vec<impl AsRef<[u8]> + Send + 'static>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
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
                    socket.write_all(response.as_ref()).unwrap();
                    String::from_utf8(request).unwrap()
                })
                .collect()
        });
        (url, worker)
    }

    fn tls_server(responses: Vec<String>) -> (Client, thread::JoinHandle<Vec<String>>) {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let certificate = include_bytes!("../tests/fixtures/http/certificate.der");
        let key = include_bytes!("../tests/fixtures/http/private-key.der");
        let config = rustls::ServerConfig::builder_with_provider(
            rustls::crypto::ring::default_provider().into(),
        )
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate.to_vec())],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key.to_vec())),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(2))
            .resolve("fixture.test", addr)
            .resolve("other.test", addr)
            .tls_certs_only([reqwest::Certificate::from_der(certificate).unwrap()])
            .build()
            .unwrap();
        let worker = thread::spawn(move || {
            let config = std::sync::Arc::new(config);
            responses
                .into_iter()
                .map(|response| {
                    let (socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let connection = rustls::ServerConnection::new(config.clone()).unwrap();
                    let mut socket = rustls::StreamOwned::new(connection, socket);
                    let mut request = Vec::new();
                    let mut byte = [0];
                    while !request.ends_with(b"\r\n\r\n") {
                        socket.read_exact(&mut byte).unwrap();
                        request.push(byte[0]);
                    }
                    socket.write_all(response.as_bytes()).unwrap();
                    socket.flush().unwrap();
                    String::from_utf8(request).unwrap()
                })
                .collect()
        });
        (client, worker)
    }

    fn slow_server(delay: Duration, responses: Vec<String>) -> (String, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let count = responses.len();
            for (index, response) in responses.into_iter().enumerate() {
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
                if index == 0 {
                    thread::sleep(delay);
                }
                let _ = socket.write_all(response.as_bytes());
            }
            count
        });
        (url, worker)
    }

    #[tokio::test]
    async fn interrupted_connections_and_bodies_are_retried_without_caching_partial_bytes() {
        let (url, worker) = server(vec![
            String::new(),
            "HTTP/1.1 200 OK\r\nContent-Length: 99\r\nConnection: close\r\n\r\n{\"partial\":"
                .into(),
            response("200 OK", "ETag: complete\r\n", r#"{"complete":true}"#),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let (snapshot, body) = fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(body, serde_json::json!({"complete":true}));
        assert_eq!(snapshot.sha256, sha256(br#"{"complete":true}"#));
        assert_eq!(
            fs::read_dir(cache.path().join("snapshots"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(worker.join().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn request_timeouts_retry_and_total_deadlines_include_waiting() {
        let (url, worker) = slow_server(
            Duration::from_millis(100),
            vec![response("200 OK", "", "{}"), response("200 OK", "", "{}")],
        );
        let cache = tempfile::tempdir().unwrap();
        let mut fetcher = Fetcher::new(cache.path()).unwrap();
        fetcher.client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(40))
            .retry(reqwest::retry::never())
            .build()
            .unwrap();
        fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(worker.join().unwrap(), 2);

        let (url, worker) = slow_server(
            Duration::from_millis(750),
            vec![response("200 OK", "", "{}")],
        );
        fetcher.client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        fetcher.total_timeout = Duration::from_millis(50);
        let started = std::time::Instant::now();
        let error = fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("total HTTP deadline"),
            "{error:#}"
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(worker.join().unwrap(), 1);

        let (url, worker) = server(vec![response(
            "429 Too Many Requests",
            "Retry-After: 1\r\n",
            "",
        )]);
        let started = std::time::Instant::now();
        let error = fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("deferred"));
        assert!(format!("{error:#}").contains("retry_at="));
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rate_limit_resets_are_respected_or_deferred() {
        let (url, worker) = server(vec![
            response(
                "429 Too Many Requests",
                "RateLimit: \"api\";r=0;t=0\r\n",
                "",
            ),
            response("200 OK", "", "{}"),
            response(
                "429 Too Many Requests",
                "RateLimit: \"api\";r=0;t=120\r\n",
                "",
            ),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        let error = fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("deferred"));
        assert!(format!("{error:#}").contains("retry_at="));
        assert_eq!(worker.join().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn document_redirects_preserve_final_urls_and_bind_304_to_the_final_target() {
        let (client, worker) = tls_server(vec![
            response(
                "301 Moved Permanently",
                "Location: https://other.test/final\r\n",
                "",
            ),
            response(
                "200 OK",
                "ETag: final\r\nContent-Type: text/plain\r\n",
                "model",
            ),
            response("302 Found", "Location: https://other.test/final\r\n", ""),
            response("304 Not Modified", "", ""),
            response("302 Found", "Location: https://other.test/changed\r\n", ""),
            response("304 Not Modified", "", ""),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let mut fetcher = Fetcher::new(cache.path()).unwrap();
        fetcher.client = client;
        let url = "https://fixture.test/model";
        let allowed = |url: &str| {
            matches!(
                Url::parse(url).unwrap().host_str(),
                Some("fixture.test" | "other.test")
            )
        };
        let first = fetcher
            .get_document_with_redirects("fixture", url, allowed)
            .await
            .unwrap();
        assert_eq!(first.0.url, "https://other.test/final");
        let receipt: Receipt = serde_json::from_slice(
            &fs::read(
                cache
                    .path()
                    .join("document-http")
                    .join(format!("{}.json", sha256(url.as_bytes()))),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(receipt.requested_url.as_deref(), Some(url));
        assert_eq!(
            fetcher
                .get_document_with_redirects("fixture", url, allowed)
                .await
                .unwrap(),
            first
        );
        let error = fetcher
            .get_document_with_redirects("fixture", url, allowed)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("304 URL does not match"));
        let requests = worker.join().unwrap();
        assert!(requests[3].contains("if-none-match: final"));
        for index in [0, 1, 2, 4, 5] {
            assert!(!requests[index].contains("if-none-match"));
        }
    }

    #[tokio::test]
    async fn document_redirects_reject_unapproved_hops_loops_and_downgrades() {
        let (client, worker) = tls_server(vec![
            response("302 Found", "Location: https://evil.test/model\r\n", ""),
            response("302 Found", "Location: http://fixture.test/model\r\n", ""),
            response("302 Found", "Location: /model\r\n", ""),
            response(
                "302 Found",
                "Location: https://user:pass@fixture.test/model\r\n",
                "",
            ),
            response("302 Found", "Location: /model#fragment\r\n", ""),
            response(
                "302 Found",
                "Location: https://fixture.test:8443/model\r\n",
                "",
            ),
            response("302 Found", "Location: /final\r\n", ""),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let mut fetcher = Fetcher::new(cache.path()).unwrap();
        fetcher.client = client;
        let url = "https://fixture.test/model";
        for expected in [
            "not allowed",
            "not allowed",
            "loop",
            "not allowed",
            "not allowed",
            "not allowed",
        ] {
            let error = fetcher
                .get_document_with_redirects("fixture", url, |url| {
                    Url::parse(url).unwrap().host_str() == Some("fixture.test")
                })
                .await
                .unwrap_err();
            assert!(format!("{error:#}").contains(expected), "{error:#}");
        }
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer fixture".parse().unwrap());
        let error = fetcher.get_json("fixture", url, headers).await.unwrap_err();
        assert!(format!("{error:#}").contains("302"));
        assert_eq!(worker.join().unwrap().len(), 7);
        assert!(!cache.path().join("document-http").exists());
    }

    #[tokio::test]
    async fn binary_pdf_documents_preserve_the_original_bytes() {
        let mut bytes = b"%PDF-1.7\r\n".to_vec();
        bytes.extend([0xff, 0xfe, 0xfd]);
        let mut raw = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/pdf\r\nConnection: close\r\n\r\n", bytes.len()).into_bytes();
        raw.extend(&bytes);
        let (url, worker) = server(vec![raw]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let (snapshot, downloaded, content_type) =
            fetcher.get_document("fixture", &url).await.unwrap();
        assert_eq!(downloaded, bytes);
        assert_eq!(snapshot.sha256, sha256(&bytes));
        assert_eq!(content_type.as_deref(), Some("application/pdf"));
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    fn response(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn documents_preserve_bytes_and_content_type_without_sharing_json_receipts() {
        let body = "\u{feff}<!doctype html>\r\n<p>模型 — pricing</p>\r\n";
        let content_type = "text/html; charset=utf-8";
        let (url, worker) = server(vec![
            response(
                "200 OK",
                &format!(
                    "ETag: document\r\nLast-Modified: Thu, 15 Oct 2026 01:00:00 GMT\r\nContent-Type: {content_type}\r\nLink: irrelevant\r\n"
                ),
                body,
            ),
            response("200 OK", "ETag: json\r\n", "{}"),
            response("304 Not Modified", "", ""),
            response("304 Not Modified", "", ""),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let document = fetcher.get_document("fixture", &url).await.unwrap();
        assert_eq!(document.1, body.as_bytes());
        assert_eq!(document.2.as_deref(), Some(content_type));
        let json = fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(
            fetcher.get_document("fixture", &url).await.unwrap(),
            document
        );
        assert_eq!(
            fetcher
                .get_json("fixture", &url, HeaderMap::new())
                .await
                .unwrap(),
            json
        );
        assert_eq!(
            fs::read(
                cache
                    .path()
                    .join("documents")
                    .join(format!("{}.bin", document.0.sha256))
            )
            .unwrap(),
            body.as_bytes()
        );
        let requests = worker.join().unwrap();
        assert!(!requests[1].contains("if-none-match"));
        assert!(requests[2].contains("if-none-match: document"));
        assert!(requests[2].contains("if-modified-since: Thu, 15 Oct 2026 01:00:00 GMT"));
        assert!(requests[3].contains("if-none-match: json"));
        assert!(
            requests
                .iter()
                .all(|request| !request.contains("authorization:"))
        );
    }

    #[tokio::test]
    async fn invalid_documents_preserve_receipts_and_cached_bytes_are_verified() {
        let body = "# Model\n\n$1 per million tokens.\n";
        let (url, worker) = server(vec![
            response(
                "200 OK",
                "ETag: good\r\nContent-Type: text/markdown\r\n",
                body,
            )
            .into_bytes(),
            response("200 OK", "", " \r\n").into_bytes(),
            response("200 OK", "Content-Type: application/zip\r\n", "bad").into_bytes(),
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 8\r\nContent-Type: application/pdf; x=\xff\r\n\r\n%PDF-1.7".to_vec(),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_DOCUMENT_BYTES + 1
            )
            .into_bytes(),
            response("304 Not Modified", "", "").into_bytes(),
            response("304 Not Modified", "", "").into_bytes(),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let first = fetcher.get_document("fixture", &url).await.unwrap();
        let receipt_path = cache
            .path()
            .join("document-http")
            .join(format!("{}.json", sha256(url.as_bytes())));
        let receipt = fs::read(&receipt_path).unwrap();
        for expected in [
            "document is empty",
            "unsupported",
            "invalid document Content-Type",
            "exceeds 8 MiB",
        ] {
            let error = fetcher.get_document("fixture", &url).await.unwrap_err();
            assert!(format!("{error:#}").contains(expected), "{error:#}");
            assert_eq!(fs::read(&receipt_path).unwrap(), receipt);
        }
        assert_eq!(fetcher.get_document("fixture", &url).await.unwrap(), first);
        let raw_path = cache
            .path()
            .join("documents")
            .join(format!("{}.bin", first.0.sha256));
        assert_eq!(fs::read(&raw_path).unwrap(), body.as_bytes());
        fs::write(raw_path, b"changed").unwrap();
        assert!(
            fetcher
                .get_document("fixture", &url)
                .await
                .unwrap_err()
                .to_string()
                .contains("SHA-256 mismatch")
        );
        assert_eq!(fs::read(&receipt_path).unwrap(), receipt);
        let requests = worker.join().unwrap();
        assert!(
            requests[1..]
                .iter()
                .all(|request| request.contains("if-none-match: good"))
        );
    }

    #[test]
    fn link_pagination_preserves_commas_quotes_and_relations() {
        let header = r#"<https://example.test/models?cursor=a,b>; title="two, \"quoted\" values"; rel="prev next", <https://example.test/first>; rel=first"#;
        assert_eq!(
            next_link(Some(header)).unwrap().as_deref(),
            Some("https://example.test/models?cursor=a,b")
        );
        assert_eq!(next_link(None).unwrap(), None);
        assert_eq!(
            next_link(Some("<https://example.test/prev>; rel=prev")).unwrap(),
            None
        );
        for header in [
            "https://example.test/next; rel=next",
            "<https://example.test/next>; rel=\"next",
            "<https://example.test/next>; rel=next,",
            "<https://example.test/next>; rel=next; rel=prev",
            "<https://example.test/a>; rel=next, <https://example.test/b>; rel=next",
            "<https://example.test/space here>; rel=next",
        ] {
            assert!(next_link(Some(header)).is_err(), "accepted {header}");
        }
    }

    #[tokio::test]
    async fn cached_link_pagination_survives_not_modified() {
        let (url, worker) = server(vec![
            response(
                "200 OK",
                "ETag: \"page\"\r\nLink: </previous>; rel=prev\r\nLink: </models?cursor=a,b>; rel=next\r\n",
                "[]",
            ),
            response("304 Not Modified", "", ""),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let first = fetcher
            .get_json_with_link("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(first.2.as_deref(), Some("/models?cursor=a,b"));
        let cached = fetcher
            .get_json_with_link("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(cached, first);
        assert!(worker.join().unwrap()[1].contains("if-none-match: \"page\""));
    }

    #[tokio::test]
    async fn not_modified_refreshes_pagination_and_cache_validators() {
        let (url, worker) = server(vec![
            response(
                "200 OK",
                "ETag: \"old\"\r\nLast-Modified: Thu, 15 Oct 2026 01:00:00 GMT\r\nLink: </models?cursor=old>; rel=next\r\n",
                "[]",
            ),
            response(
                "304 Not Modified",
                "ETag: \"new\"\r\nLast-Modified: Fri, 16 Oct 2026 01:00:00 GMT\r\nLink: </models?cursor=new>; rel=next\r\n",
                "",
            ),
            response("304 Not Modified", "Link: invalid\r\n", ""),
            response("304 Not Modified", "", ""),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let first = fetcher
            .get_json_with_link("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        let refreshed = fetcher
            .get_json_with_link("fixture", &url, HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(refreshed.0, first.0);
        assert_eq!(refreshed.1, first.1);
        assert_eq!(refreshed.2.as_deref(), Some("/models?cursor=new"));
        assert!(
            fetcher
                .get_json_with_link("fixture", &url, HeaderMap::new())
                .await
                .unwrap_err()
                .to_string()
                .contains("invalid Link")
        );
        assert_eq!(
            fetcher
                .get_json_with_link("fixture", &url, HeaderMap::new())
                .await
                .unwrap(),
            refreshed
        );
        let requests = worker.join().unwrap();
        assert!(requests[1].contains("if-none-match: \"old\""));
        for request in &requests[2..] {
            assert!(request.contains("if-none-match: \"new\""));
            assert!(request.contains("if-modified-since: Fri, 16 Oct 2026 01:00:00 GMT"));
        }
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
        let error = fetcher
            .get_json("fixture", &url, HeaderMap::new())
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("retry limit"));
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
    fn rate_limit_parameters_require_valid_exhausted_quota_reset_times() {
        let now = OffsetDateTime::parse("2026-10-15T01:00:00Z", &Rfc3339).unwrap();
        let mut headers = HeaderMap::new();
        for (value, expected) in [
            (r#""api";r=0;t=12"#, 12),
            (r#""api";r=1;t=120"#, 1),
            (r#""api";r=0;t=2, "resolvers";r=0;t=8"#, 8),
        ] {
            headers.insert("ratelimit", value.parse().unwrap());
            assert_eq!(
                retry_delay(&headers, 0, now, RETRY_BUDGET).unwrap(),
                Duration::from_secs(expected)
            );
        }
        for value in [
            r#""api";r=0"#,
            r#""api";r=0;t=invalid"#,
            r#""api";r=0;t=1;t=2"#,
        ] {
            headers.insert("ratelimit", value.parse().unwrap());
            assert!(retry_delay(&headers, 0, now, RETRY_BUDGET).is_err());
        }
        headers.insert("ratelimit", r#""api";r=0;t=120"#.parse().unwrap());
        let error = retry_delay(&headers, 0, now, RETRY_BUDGET).unwrap_err();
        assert!(error.to_string().contains("retry_at=2026-10-15T01:02:00Z"));
        headers.insert(RETRY_AFTER, "3".parse().unwrap());
        assert_eq!(
            retry_delay(&headers, 0, now, RETRY_BUDGET).unwrap(),
            Duration::from_secs(3)
        );
    }

    #[tokio::test]
    async fn document_redirects_enforce_every_hop_and_the_hop_limit() {
        let mut responses = vec![
            response("302 Found", "Location: /allowed\r\n", ""),
            response("302 Found", "Location: https://evil.test/model\r\n", ""),
        ];
        responses.extend(
            (0..=MAX_REDIRECTS)
                .map(|index| response("302 Found", &format!("Location: /page-{index}\r\n"), "")),
        );
        let (client, worker) = tls_server(responses);
        let cache = tempfile::tempdir().unwrap();
        let mut fetcher = Fetcher::new(cache.path()).unwrap();
        fetcher.client = client;
        let allowed = |url: &str| Url::parse(url).unwrap().host_str() == Some("fixture.test");
        let error = fetcher
            .get_document_with_redirects("fixture", "https://fixture.test/model", allowed)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not allowed"));
        let error = fetcher
            .get_document_with_redirects("fixture", "https://fixture.test/model", allowed)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("redirect limit"));
        assert_eq!(worker.join().unwrap().len(), MAX_REDIRECTS + 3);
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

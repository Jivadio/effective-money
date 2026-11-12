// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`HttpClient`]: the only HTTP path for bank connectors.
//!
//! - Logs method, host and a redacted path only (segments with 5+ digits or
//!   shaped like an IBAN become `{id}`, see [`redact_path`]): never the query
//!   string, bodies or header values.
//! - Calls an optional [`RateLimiter`] before every request.
//! - Caps response bodies at [`MAX_BODY_BYTES`], enforced while streaming.
//! - Three modes ([`HttpMode`]):
//!   - `Live`: real network (HTTPS only, unless explicitly allowed for tests);
//!   - `Replay`: answers from a [`FixtureSet`], fully offline and
//!     deterministic;
//!   - `Record`: real network, and every exchange is written to a fixture
//!     directory after going through a [`Scrubber`]. Request bodies and
//!     request headers are never recorded.
//! - Redirects are not followed: the connector sees the `3xx` and its
//!   `Location`, so Live, Record and Replay behave the same.
//! - No cookie jar: read `set-cookie` from the response and send `cookie`
//!   yourself (keep what you need in the `SessionBlob`).
//! - A `429` response becomes [`ConnectorError::RateLimited`].
//!
//! Request header values, request bodies and response header values and
//! bodies are kept in zeroized buffers. The copies made inside reqwest and
//! hyper while a request is in flight cannot be wiped.
//!
//! # Fixture format
//!
//! One JSON file per exchange, loaded in file-name order:
//!
//! ```json
//! {
//!   "request": { "method": "GET", "path": "/api/accounts", "query": "page=2" },
//!   "response": {
//!     "status": 200,
//!     "headers": { "content-type": "application/json" },
//!     "body": { "accounts": [] }
//!   }
//! }
//! ```
//!
//! Requests match on method, path and query (exact; the host is ignored).
//! Query parameters such as cache-busters can be left out of the match with
//! [`FixtureSet::ignore_query_params`]. To tell apart two requests to the
//! same URL, a hand-written fixture may add `"body_sha256": "<hex>"` to its
//! `request`: it then only answers a request whose body has that SHA-256
//! (fixtures without it answer any body). The recorder never writes it, since
//! a hash of a body holding a password could be brute-forced.
//!
//! A `body` that is a JSON string is served as raw text; any other JSON value
//! is served serialized. When several files match the same request they are
//! served in order. Running out of fixtures is an error (strict mode), unless
//! [`FixtureSet::repeat_last`] is set.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub use reqwest::Url;

use crate::connector::ConnectorError;
use crate::scrub::Scrubber;
use crate::secret::SecretString;

/// Default request timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Largest response body accepted (16 MiB).
pub const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        }
    }

    fn to_reqwest(self) -> reqwest::Method {
        match self {
            Self::Get => reqwest::Method::GET,
            Self::Post => reqwest::Method::POST,
            Self::Put => reqwest::Method::PUT,
            Self::Patch => reqwest::Method::PATCH,
            Self::Delete => reqwest::Method::DELETE,
            Self::Head => reqwest::Method::HEAD,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Replaces path segments that look like identifiers with `{id}`, for logs
/// and error messages: 5 or more digits, shaped like an IBAN, an e-mail
/// address, or an opaque token (16+ characters mixing letters and digits).
///
/// ```
/// use solde_connector_sdk::http::redact_path;
/// assert_eq!(
///     redact_path("/api/accounts/12345678901/transactions"),
///     "/api/accounts/{id}/transactions"
/// );
/// ```
pub fn redact_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            let digits = segment.bytes().filter(u8::is_ascii_digit).count();
            let letters = segment.bytes().filter(u8::is_ascii_alphabetic).count();
            let email = segment.contains('@') || segment.to_ascii_lowercase().contains("%40");
            let token = segment.len() >= 16 && digits > 0 && letters > 0;
            if digits >= 5 || looks_like_iban(segment) || email || token {
                "{id}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn looks_like_iban(segment: &str) -> bool {
    let b = segment.as_bytes();
    b.len() >= 14
        && b.len() <= 34
        && b[..2].iter().all(u8::is_ascii_alphabetic)
        && b[2..4].iter().all(u8::is_ascii_digit)
        && b[4..].iter().all(u8::is_ascii_alphanumeric)
}

/// An outgoing request. Header values and the body are treated as secrets
/// (zeroized, never logged), so this type has no `Debug` or `Clone`.
pub struct HttpRequest {
    method: Method,
    url: Url,
    headers: Vec<(String, SecretString)>,
    body: Option<Zeroizing<Vec<u8>>>,
    sca: bool,
}

impl HttpRequest {
    pub fn new(method: Method, url: Url) -> Self {
        Self {
            method,
            url,
            headers: Vec::new(),
            body: None,
            sca: false,
        }
    }

    pub fn get(url: Url) -> Self {
        Self::new(Method::Get, url)
    }

    pub fn post(url: Url) -> Self {
        Self::new(Method::Post, url)
    }

    /// Adds a header. Values are never logged nor recorded.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .push((name.to_owned(), SecretString::new(value.to_owned())));
        self
    }

    /// Marks a request that can make the bank send an OTP, an SMS or a push
    /// notification (typically a password login). It can then only be sent
    /// with [`crate::LoginContext::send`], which refuses it in Unattended
    /// mode.
    pub fn triggers_sca(mut self) -> Self {
        self.sca = true;
        self
    }

    pub fn is_sca_trigger(&self) -> bool {
        self.sca
    }

    /// `Authorization: Bearer <token>`.
    pub fn bearer(self, token: &str) -> Self {
        let value = Zeroizing::new(format!("Bearer {token}"));
        self.header("authorization", &value)
    }

    /// Raw body with its content type.
    pub fn body(mut self, content_type: &str, bytes: Vec<u8>) -> Self {
        self = self.header("content-type", content_type);
        self.body = Some(Zeroizing::new(bytes));
        self
    }

    /// JSON body.
    pub fn json<T: Serialize + ?Sized>(self, value: &T) -> Result<Self, ConnectorError> {
        let bytes = serde_json::to_vec(value)
            .map_err(|_| ConnectorError::broken("cannot encode JSON request body"))?;
        Ok(self.body("application/json", bytes))
    }

    /// `application/x-www-form-urlencoded` body.
    pub fn form(self, pairs: &[(&str, &str)]) -> Self {
        let mut out = Zeroizing::new(String::new());
        for (i, (k, v)) in pairs.iter().enumerate() {
            if i > 0 {
                out.push('&');
            }
            form_encode(k, &mut out);
            out.push('=');
            form_encode(v, &mut out);
        }
        let bytes = out.as_bytes().to_vec();
        self.body("application/x-www-form-urlencoded", bytes)
    }

    pub fn method(&self) -> Method {
        self.method
    }

    pub fn url(&self) -> &Url {
        &self.url
    }
}

fn form_encode(input: &str, out: &mut String) {
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'*' => {
                out.push(char::from(b));
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
}

/// A response. Header values and the body may hold tokens or cookies, so
/// they are zeroized on drop and `Debug` shows the status only.
pub struct HttpResponse {
    status: u16,
    headers: Vec<(String, SecretString)>,
    body: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl HttpResponse {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// First value of a header (case-insensitive name). May be secret
    /// (`set-cookie`): never log it.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.header_all(name).next()
    }

    /// Every value of a header, e.g. `set-cookie`.
    pub fn header_all<'a, 'n>(
        &'a self,
        name: &'n str,
    ) -> impl Iterator<Item = &'a str> + use<'a, 'n> {
        self.headers
            .iter()
            .filter(move |(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.expose())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn text(&self) -> Result<&str, ConnectorError> {
        std::str::from_utf8(&self.body)
            .map_err(|_| ConnectorError::broken("response body is not UTF-8"))
    }

    /// Parses the body as JSON. Errors never quote the body.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, ConnectorError> {
        serde_json::from_slice(&self.body).map_err(|e| {
            ConnectorError::broken(format!(
                "unexpected JSON response (line {}, column {})",
                e.line(),
                e.column()
            ))
        })
    }
}

/// Called before every request; may wait, or refuse with
/// [`ConnectorError::RateLimited`]. One limiter per bank connection.
#[async_trait]
pub trait RateLimiter: Send + Sync {
    async fn acquire(&self, host: &str) -> Result<(), ConnectorError>;
}

/// Spaces requests at least `interval` apart.
pub struct MinInterval {
    interval: Duration,
    next: Mutex<Option<Instant>>,
}

impl MinInterval {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: Mutex::new(None),
        }
    }
}

#[async_trait]
impl RateLimiter for MinInterval {
    async fn acquire(&self, _host: &str) -> Result<(), ConnectorError> {
        let slot = {
            let mut next = self
                .next
                .lock()
                .map_err(|_| ConnectorError::broken("rate limiter poisoned"))?;
            let now = Instant::now();
            let slot = next.map_or(now, |n| n.max(now));
            *next = Some(slot + self.interval);
            slot
        };
        tokio::time::sleep_until(slot.into()).await;
        Ok(())
    }
}

/// Where responses come from.
#[non_exhaustive]
// Built once per client; boxing the scrubber would only complicate the API.
#[allow(clippy::large_enum_variant)]
pub enum HttpMode {
    Live,
    Replay(FixtureSet),
    /// Live, and each exchange, scrubbed by `scrubber`, is written to `dir`.
    Record {
        dir: PathBuf,
        scrubber: Scrubber,
    },
}

/// [`HttpClient`] settings. Start from [`HttpConfig::live`],
/// [`HttpConfig::replay`] or [`HttpConfig::record`].
#[non_exhaustive]
pub struct HttpConfig {
    pub mode: HttpMode,
    pub user_agent: Option<String>,
    pub timeout: Duration,
    pub rate_limiter: Option<Arc<dyn RateLimiter>>,
    /// Allow plain `http://` URLs (local test servers only). Default: false.
    pub allow_plain_http: bool,
}

impl HttpConfig {
    pub fn new(mode: HttpMode) -> Self {
        Self {
            mode,
            user_agent: None,
            timeout: DEFAULT_TIMEOUT,
            rate_limiter: None,
            allow_plain_http: false,
        }
    }

    pub fn live() -> Self {
        Self::new(HttpMode::Live)
    }

    pub fn replay(fixtures: FixtureSet) -> Self {
        Self::new(HttpMode::Replay(fixtures))
    }

    /// Record mode with the default [`Scrubber`].
    pub fn record(dir: impl Into<PathBuf>) -> Result<Self, ConnectorError> {
        Ok(Self::record_with(dir, Scrubber::new()?))
    }

    /// Record mode with your own scrubber (add bank keys with
    /// [`Scrubber::with_key`]).
    pub fn record_with(dir: impl Into<PathBuf>, scrubber: Scrubber) -> Self {
        Self::new(HttpMode::Record {
            dir: dir.into(),
            scrubber,
        })
    }

    pub fn with_rate_limiter(mut self, limiter: Arc<dyn RateLimiter>) -> Self {
        self.rate_limiter = Some(limiter);
        self
    }

    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Allows plain `http://` (local test servers only).
    pub fn with_plain_http(mut self) -> Self {
        self.allow_plain_http = true;
        self
    }
}

/// A request as seen by tests: method and path only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSummary {
    pub method: Method,
    pub path: String,
}

/// The HTTP client connectors must use. Cheap to clone (shared state).
#[derive(Clone)]
pub struct HttpClient {
    inner: Arc<Inner>,
}

struct Inner {
    backend: Backend,
    rate_limiter: Option<Arc<dyn RateLimiter>>,
    allow_plain_http: bool,
    sent: Mutex<Vec<RequestSummary>>,
}

enum Backend {
    Live(reqwest::Client),
    Replay(Replayer),
    Record(reqwest::Client, Box<Recorder>),
}

impl HttpClient {
    pub fn new(config: HttpConfig) -> Result<Self, ConnectorError> {
        let backend = match config.mode {
            HttpMode::Live => Backend::Live(build_client(config.user_agent, config.timeout)?),
            HttpMode::Replay(set) => Backend::Replay(Replayer {
                set,
                cursor: Mutex::new(BTreeMap::new()),
            }),
            HttpMode::Record { dir, scrubber } => Backend::Record(
                build_client(config.user_agent, config.timeout)?,
                Box::new(Recorder {
                    dir,
                    scrubber: Mutex::new(scrubber),
                    seq: AtomicUsize::new(0),
                }),
            ),
        };
        Ok(Self {
            inner: Arc::new(Inner {
                backend,
                rate_limiter: config.rate_limiter,
                allow_plain_http: config.allow_plain_http,
                sent: Mutex::new(Vec::new()),
            }),
        })
    }

    /// Shorthand for a replay client.
    pub fn replay(fixtures: FixtureSet) -> Result<Self, ConnectorError> {
        Self::new(HttpConfig::replay(fixtures))
    }

    /// Every request sent so far (method and unredacted path), oldest first.
    /// For tests.
    pub fn requests(&self) -> Vec<RequestSummary> {
        self.inner
            .sent
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn request_count(&self) -> usize {
        self.inner.sent.lock().map(|s| s.len()).unwrap_or_default()
    }

    /// Sends a request. Any HTTP status is returned as `Ok` except `429`
    /// ([`ConnectorError::RateLimited`]). Requests marked
    /// [`HttpRequest::triggers_sca`] are refused: send them with
    /// [`crate::LoginContext::send`].
    pub async fn send(&self, req: HttpRequest) -> Result<HttpResponse, ConnectorError> {
        if req.sca {
            return Err(ConnectorError::broken(
                "an SCA-triggering request must be sent with LoginContext::send",
            ));
        }
        self.send_checked(req).await
    }

    /// [`HttpClient::send`] without the SCA check (the login context did it).
    pub(crate) async fn send_checked(
        &self,
        req: HttpRequest,
    ) -> Result<HttpResponse, ConnectorError> {
        let host = req.url.host_str().unwrap_or_default().to_owned();
        let path = req.url.path().to_owned();
        let shown = redact_path(&path);
        if !matches!(self.inner.backend, Backend::Replay(_))
            && req.url.scheme() != "https"
            && !self.inner.allow_plain_http
        {
            return Err(ConnectorError::broken("refusing a non-HTTPS URL"));
        }
        if let Some(limiter) = &self.inner.rate_limiter {
            limiter.acquire(&host).await?;
        }
        if let Ok(mut sent) = self.inner.sent.lock() {
            sent.push(RequestSummary {
                method: req.method,
                path,
            });
        }
        let method = req.method;
        tracing::debug!(method = %method, host = %host, path = %shown, "http request");
        let resp = match &self.inner.backend {
            Backend::Live(client) => execute(client, req).await?,
            Backend::Replay(replayer) => replayer.serve(&req)?,
            Backend::Record(client, recorder) => {
                let url = req.url.clone();
                let resp = execute(client, req).await?;
                recorder.write(method, &url, &resp)?;
                resp
            }
        };
        tracing::debug!(method = %method, host = %host, path = %shown, status = resp.status, "http response");
        if resp.status == 429 {
            let retry_after = resp
                .header("retry-after")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            return Err(ConnectorError::RateLimited { retry_after });
        }
        Ok(resp)
    }
}

/// rustls with the ring provider and the Mozilla root store (webpki-roots),
/// independent of the host's certificate store and of any process-wide
/// default provider.
pub(crate) fn tls_config() -> Result<rustls::ClientConfig, ConnectorError> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| ConnectorError::broken("cannot configure TLS"))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

fn build_client(
    user_agent: Option<String>,
    timeout: Duration,
) -> Result<reqwest::Client, ConnectorError> {
    let mut builder = reqwest::Client::builder()
        .tls_backend_preconfigured(tls_config()?)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout);
    if let Some(ua) = user_agent {
        builder = builder.user_agent(ua);
    }
    builder
        .build()
        .map_err(|_| ConnectorError::broken("cannot build the HTTP client"))
}

/// Maps a transport error without its URL (which could carry a query string).
pub(crate) fn network_error(err: &reqwest::Error) -> ConnectorError {
    let what = if err.is_timeout() {
        "timeout"
    } else if err.is_connect() {
        "connection failed"
    } else if err.is_body() || err.is_decode() {
        "invalid response body"
    } else {
        "request failed"
    };
    ConnectorError::Network(what.to_owned())
}

/// Reads a response body into a zeroized buffer, failing as soon as it grows
/// past [`MAX_BODY_BYTES`].
pub(crate) async fn read_capped(
    mut resp: reqwest::Response,
) -> Result<Zeroizing<Vec<u8>>, ConnectorError> {
    if resp
        .content_length()
        .is_some_and(|n| n > MAX_BODY_BYTES as u64)
    {
        return Err(ConnectorError::broken("response body too large"));
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = resp.chunk().await.map_err(|e| network_error(&e))? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(ConnectorError::broken("response body too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn execute(
    client: &reqwest::Client,
    req: HttpRequest,
) -> Result<HttpResponse, ConnectorError> {
    let HttpRequest {
        method,
        url,
        headers,
        body,
        ..
    } = req;
    let mut builder = client.request(method.to_reqwest(), url);
    for (name, value) in &headers {
        let mut value = reqwest::header::HeaderValue::from_str(value.expose())
            .map_err(|_| ConnectorError::broken("invalid header value"))?;
        // Keeps the value out of HTTP/2 header compression tables and debug output.
        value.set_sensitive(true);
        builder = builder.header(name.as_str(), value);
    }
    if let Some(mut body) = body {
        // Hand the buffer itself to reqwest (no extra copy). reqwest's own
        // buffers cannot be wiped; our emptied wrapper is.
        builder = builder.body(std::mem::take(&mut *body));
    }
    let resp = builder.send().await.map_err(|e| network_error(&e))?;
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            Some((
                k.as_str().to_owned(),
                SecretString::new(v.to_str().ok()?.to_owned()),
            ))
        })
        .collect();
    let body = read_capped(resp).await?;
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

/// Fixtures loaded for [`HttpMode::Replay`].
#[derive(Debug, Clone, Default)]
pub struct FixtureSet {
    entries: BTreeMap<FixtureKey, Vec<Fixture>>,
    repeat_last: bool,
    ignored_query: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FixtureKey {
    method: String,
    path: String,
    query: Option<String>,
    body_sha256: Option<String>,
}

#[derive(Debug, Clone)]
struct Fixture {
    /// `(directory index, file index)`: serving order after merges.
    seq: (usize, usize),
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct FixtureFile {
    request: FixtureRequest,
    response: FixtureResponse,
}

#[derive(Serialize, Deserialize)]
struct FixtureRequest {
    method: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body_sha256: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct FixtureResponse {
    status: u16,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    headers: BTreeMap<String, HeaderValues>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body: Option<Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum HeaderValues {
    One(String),
    Many(Vec<String>),
}

impl FixtureSet {
    /// Loads every `*.json` file of `dir` (not recursive), in file-name order.
    pub fn from_dir(dir: impl AsRef<Path>) -> Result<Self, ConnectorError> {
        Self::from_dirs(&[dir])
    }

    /// Loads several directories; a later directory replaces the fixtures of
    /// an earlier one for the requests it defines (e.g. `day1/` then `day2/`).
    pub fn from_dirs(dirs: &[impl AsRef<Path>]) -> Result<Self, ConnectorError> {
        let mut set = Self::default();
        for (layer_index, dir) in dirs.iter().enumerate() {
            let mut files: Vec<(String, String)> = Vec::new();
            let entries = std::fs::read_dir(dir.as_ref())
                .map_err(|_| ConnectorError::broken("cannot read the fixture directory"))?;
            for entry in entries {
                let path = entry
                    .map_err(|_| ConnectorError::broken("cannot read the fixture directory"))?
                    .path();
                if path.extension().is_some_and(|e| e == "json") {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                    let text = std::fs::read_to_string(&path)
                        .map_err(|_| ConnectorError::broken("cannot read a fixture file"))?;
                    files.push((name.unwrap_or_default(), text));
                }
            }
            files.sort();
            let layer = Self::parse(
                layer_index,
                files.iter().map(|(n, t)| (n.as_str(), t.as_str())),
            )?;
            set.entries.extend(layer.entries);
        }
        Ok(set)
    }

    /// Builds a set from `(file name, JSON)` pairs, e.g. `include_str!`ed
    /// fixtures. Pairs are sorted by name.
    pub fn from_static(files: &[(&str, &str)]) -> Result<Self, ConnectorError> {
        let mut files = files.to_vec();
        files.sort();
        Self::parse(0, files.into_iter())
    }

    /// When the fixtures for a request run out, serve the last one again
    /// instead of failing. Off by default (strict).
    pub fn repeat_last(mut self) -> Self {
        self.repeat_last = true;
        self
    }

    /// Leaves these query parameters (cache-busters, timestamps) out of the
    /// match, in fixtures and in requests.
    pub fn ignore_query_params(mut self, names: &[&str]) -> Self {
        self.ignored_query
            .extend(names.iter().map(|n| (*n).to_owned()));
        let old = std::mem::take(&mut self.entries);
        for (mut key, fixtures) in old {
            key.query = normalize_query(key.query.as_deref(), &self.ignored_query);
            self.entries.entry(key).or_default().extend(fixtures);
        }
        // Merged entries keep the file-name order.
        for fixtures in self.entries.values_mut() {
            fixtures.sort_by_key(|f| f.seq);
        }
        self
    }

    fn parse<'a>(
        layer: usize,
        files: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, ConnectorError> {
        let mut set = Self::default();
        for (file_index, (name, text)) in files.enumerate() {
            let file: FixtureFile = serde_json::from_str(text)
                .map_err(|e| ConnectorError::broken(format!("invalid fixture {name}: {e}")))?;
            let key = FixtureKey {
                method: file.request.method.to_ascii_uppercase(),
                path: file.request.path,
                query: file.request.query.filter(|q| !q.is_empty()),
                body_sha256: file.request.body_sha256.map(|h| h.to_ascii_lowercase()),
            };
            let body = match file.response.body {
                None => Vec::new(),
                Some(Value::String(s)) => s.into_bytes(),
                Some(v) => serde_json::to_vec(&v)
                    .map_err(|_| ConnectorError::broken(format!("invalid fixture {name}")))?,
            };
            let mut headers = Vec::new();
            for (k, v) in file.response.headers {
                match v {
                    HeaderValues::One(v) => headers.push((k, v)),
                    HeaderValues::Many(vs) => {
                        headers.extend(vs.into_iter().map(|v| (k.clone(), v)));
                    }
                }
            }
            set.entries.entry(key).or_default().push(Fixture {
                seq: (layer, file_index),
                status: file.response.status,
                headers,
                body,
            });
        }
        Ok(set)
    }

    /// Number of distinct requests this set can answer.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The query without the ignored parameters (`None` if nothing is left).
fn normalize_query(query: Option<&str>, ignored: &[String]) -> Option<String> {
    let query = query.filter(|q| !q.is_empty())?;
    if ignored.is_empty() {
        return Some(query.to_owned());
    }
    let parsed = Url::parse(&format!("http://replay.invalid/?{query}")).ok()?;
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .into_owned()
        .filter(|(k, _)| !ignored.contains(k))
        .collect();
    if kept.is_empty() {
        return None;
    }
    let mut out = parsed.clone();
    out.query_pairs_mut().clear().extend_pairs(kept);
    out.query().map(str::to_owned)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

struct Replayer {
    set: FixtureSet,
    cursor: Mutex<BTreeMap<FixtureKey, usize>>,
}

impl Replayer {
    fn serve(&self, req: &HttpRequest) -> Result<HttpResponse, ConnectorError> {
        let mut key = FixtureKey {
            method: req.method.as_str().to_owned(),
            path: req.url.path().to_owned(),
            query: normalize_query(req.url.query(), &self.set.ignored_query),
            body_sha256: req.body.as_ref().map(|b| sha256_hex(b)),
        };
        if key.body_sha256.is_some() && !self.set.entries.contains_key(&key) {
            key.body_sha256 = None;
        }
        let shown = || format!("{} {}", key.method, redact_path(&key.path));
        let Some(fixtures) = self.set.entries.get(&key) else {
            return Err(ConnectorError::Network(format!(
                "replay: no fixture for {}",
                shown()
            )));
        };
        let mut cursor = self
            .cursor
            .lock()
            .map_err(|_| ConnectorError::broken("replay state poisoned"))?;
        let index = cursor.entry(key.clone()).or_insert(0);
        let fixture = match fixtures.get(*index) {
            Some(f) => f,
            None if self.set.repeat_last => fixtures
                .last()
                .ok_or_else(|| ConnectorError::broken("empty fixture entry"))?,
            None => {
                return Err(ConnectorError::Network(format!(
                    "replay: fixtures exhausted for {} (call {})",
                    shown(),
                    *index + 1
                )));
            }
        };
        *index += 1;
        Ok(HttpResponse {
            status: fixture.status,
            headers: fixture
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), SecretString::new(v.clone())))
                .collect(),
            body: Zeroizing::new(fixture.body.clone()),
        })
    }
}

struct Recorder {
    dir: PathBuf,
    scrubber: Mutex<Scrubber>,
    seq: AtomicUsize,
}

/// Response headers kept in recorded fixtures. Everything else is dropped.
const RECORDED_HEADERS: &[&str] = &["content-type", "location", "retry-after", "set-cookie"];

impl Recorder {
    fn write(&self, method: Method, url: &Url, resp: &HttpResponse) -> Result<(), ConnectorError> {
        let mut scrubber = self
            .scrubber
            .lock()
            .map_err(|_| ConnectorError::broken("recorder state poisoned"))?;
        // The body first: values it maps (account numbers, tokens) are then
        // replaced in the URL and headers too.
        let body = if resp.bytes().is_empty() {
            None
        } else if let Ok(mut json) = serde_json::from_slice::<Value>(resp.bytes()) {
            scrubber.scrub_json(&mut json);
            Some(json)
        } else {
            let text = Zeroizing::new(String::from_utf8_lossy(resp.bytes()).into_owned());
            Some(Value::String(scrubber.scrub_text(&text)))
        };
        let path = scrubber.scrub_path(url.path());
        let query = url.query().map(|q| scrubber.scrub_query(q));
        let mut headers: BTreeMap<String, HeaderValues> = BTreeMap::new();
        for name in RECORDED_HEADERS {
            let values: Vec<String> = resp
                .header_all(name)
                .map(|v| match *name {
                    // Keep the cookie name only.
                    "set-cookie" => {
                        let cookie = v.split(';').next().unwrap_or_default();
                        let name = cookie.split('=').next().unwrap_or_default();
                        format!("{}=SCRUBBED", scrubber.scrub_text(name))
                    }
                    "location" => scrubber.scrub_location(v, url),
                    _ => scrubber.scrub_text(v),
                })
                .collect();
            match values.len() {
                0 => {}
                1 => {
                    headers.insert((*name).to_owned(), HeaderValues::One(values.concat()));
                }
                _ => {
                    headers.insert((*name).to_owned(), HeaderValues::Many(values));
                }
            }
        }
        let file = FixtureFile {
            request: FixtureRequest {
                method: method.as_str().to_owned(),
                path: path.clone(),
                query,
                body_sha256: None,
            },
            response: FixtureResponse {
                status: resp.status,
                headers,
                body,
            },
        };
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let slug: String = path
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .trim_matches('-')
            .chars()
            .take(60)
            .collect();
        let name = format!(
            "{seq:04}-{}-{slug}.json",
            method.as_str().to_ascii_lowercase()
        );
        let json = serde_json::to_string_pretty(&file)
            .map_err(|_| ConnectorError::broken("cannot encode a fixture"))?;
        std::fs::create_dir_all(&self.dir)
            .and_then(|()| std::fs::write(self.dir.join(name), json + "\n"))
            .map_err(|_| ConnectorError::broken("cannot write a fixture"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(&format!("https://any.invalid{s}")).unwrap()
    }

    #[test]
    fn form_encoding() {
        let req = HttpRequest::post(url("/login")).form(&[("user", "jane doe"), ("p", "a&b=c/é")]);
        let body = req.body.unwrap();
        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            "user=jane+doe&p=a%26b%3Dc%2F%C3%A9"
        );
    }

    #[test]
    fn path_redaction() {
        assert_eq!(
            redact_path("/api/accounts/CHK-001/tx"),
            "/api/accounts/CHK-001/tx"
        );
        assert_eq!(redact_path("/a/12345/b"), "/a/{id}/b");
        assert_eq!(
            redact_path("/iban/FR7630006000011234567890189"),
            "/iban/{id}"
        );
        assert_eq!(
            redact_path("/v1/sessions/s-1/actions"),
            "/v1/sessions/s-1/actions"
        );
    }

    fn set(files: &[(&str, &str)]) -> FixtureSet {
        FixtureSet::from_static(files).unwrap()
    }

    const PAGE_1A: &str = r#"{"request":{"method":"GET","path":"/a","query":"page=1"},
        "response":{"status":200,"body":{"n":1}}}"#;
    const PAGE_1B: &str = r#"{"request":{"method":"GET","path":"/a","query":"page=1"},
        "response":{"status":200,"headers":{"set-cookie":["a=1","b=2"]},"body":"second"}}"#;

    #[tokio::test]
    async fn replay_is_strict_by_default() {
        let http = HttpClient::replay(set(&[
            ("1.json", PAGE_1A),
            ("2.json", PAGE_1B),
            (
                "3.json",
                r#"{"request":{"method":"get","path":"/a"},"response":{"status":204}}"#,
            ),
        ]))
        .unwrap();

        let r1 = http.send(HttpRequest::get(url("/a?page=1"))).await.unwrap();
        assert_eq!(r1.json::<Value>().unwrap()["n"], 1);
        let r2 = http.send(HttpRequest::get(url("/a?page=1"))).await.unwrap();
        assert_eq!(r2.text().unwrap(), "second");
        assert_eq!(
            r2.header_all("Set-Cookie").collect::<Vec<_>>(),
            ["a=1", "b=2"]
        );
        let exhausted = http.send(HttpRequest::get(url("/a?page=1"))).await;
        assert!(
            matches!(&exhausted, Err(ConnectorError::Network(m)) if m.contains("exhausted")),
            "{exhausted:?}"
        );
        assert_eq!(
            http.send(HttpRequest::get(url("/a")))
                .await
                .unwrap()
                .status(),
            204
        );

        let missing = http.send(HttpRequest::post(url("/a/123456"))).await;
        assert_eq!(
            missing.unwrap_err(),
            ConnectorError::Network("replay: no fixture for POST /a/{id}".to_owned())
        );
        assert_eq!(http.request_count(), 5);
        assert_eq!(
            http.requests()[0],
            RequestSummary {
                method: Method::Get,
                path: "/a".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn replay_repeat_last_and_ignored_params() {
        let http = HttpClient::replay(
            set(&[(
                "1.json",
                r#"{"request":{"method":"GET","path":"/a","query":"page=1&_=111"},
                    "response":{"status":200,"body":"ok"}}"#,
            )])
            .ignore_query_params(&["_"])
            .repeat_last(),
        )
        .unwrap();
        for buster in ["222", "333"] {
            let r = http
                .send(HttpRequest::get(url(&format!("/a?_={buster}&page=1"))))
                .await
                .unwrap();
            assert_eq!(r.text().unwrap(), "ok");
        }
    }

    #[tokio::test]
    async fn ignored_params_keep_file_order() {
        // Without the sequence, "_=a" (2.json) would sort before "_=b" (1.json).
        let http = HttpClient::replay(
            set(&[
                (
                    "1.json",
                    r#"{"request":{"method":"GET","path":"/a","query":"_=b"},"response":{"status":200,"body":"first"}}"#,
                ),
                (
                    "2.json",
                    r#"{"request":{"method":"GET","path":"/a","query":"_=a"},"response":{"status":200,"body":"second"}}"#,
                ),
            ])
            .ignore_query_params(&["_"]),
        )
        .unwrap();
        for expected in ["first", "second"] {
            let r = http.send(HttpRequest::get(url("/a?_=zzz"))).await.unwrap();
            assert_eq!(r.text().unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn replay_matches_body_hash() {
        let hash = sha256_hex(br#"{"step":2}"#);
        let step2 = format!(
            r#"{{"request":{{"method":"POST","path":"/login","body_sha256":"{hash}"}},
                "response":{{"status":200,"body":"two"}}}}"#
        );
        let http = HttpClient::replay(set(&[
            (
                "1.json",
                r#"{"request":{"method":"POST","path":"/login"},"response":{"status":200,"body":"any"}}"#,
            ),
            ("2.json", &step2),
        ]))
        .unwrap();
        let send = |step: u32| {
            let http = http.clone();
            async move {
                let req = HttpRequest::post(url("/login"))
                    .json(&serde_json::json!({ "step": step }))
                    .unwrap();
                http.send(req).await.unwrap().text().unwrap().to_owned()
            }
        };
        assert_eq!(send(2).await, "two");
        assert_eq!(send(1).await, "any");
    }

    #[tokio::test]
    async fn replay_429_is_rate_limited() {
        let http = HttpClient::replay(set(&[(
            "1.json",
            r#"{"request":{"method":"GET","path":"/x"},
                "response":{"status":429,"headers":{"retry-after":"12"}}}"#,
        )]))
        .unwrap();
        let r = http.send(HttpRequest::get(url("/x"))).await;
        assert_eq!(
            r.unwrap_err(),
            ConnectorError::RateLimited {
                retry_after: Some(Duration::from_secs(12))
            }
        );
    }

    #[tokio::test]
    async fn live_refuses_plain_http() {
        let http = HttpClient::new(HttpConfig::live()).unwrap();
        let r = http
            .send(HttpRequest::get(Url::parse("http://127.0.0.1:9/").unwrap()))
            .await;
        assert!(matches!(r, Err(ConnectorError::Broken { .. })));
        assert_eq!(http.request_count(), 0);
    }

    #[tokio::test]
    async fn min_interval_spaces_requests() {
        let limiter = MinInterval::new(Duration::from_millis(20));
        let start = Instant::now();
        for _ in 0..3 {
            limiter.acquire("bank.invalid").await.unwrap();
        }
        assert!(start.elapsed() >= Duration::from_millis(40));
    }
}

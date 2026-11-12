// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`BrowserClient`]: client for the browser-helper service (protocol v1).
//!
//! The helper drives a real browser for the login steps where a bank demands
//! one. It knows no bank: the connector sends generic actions, reads the
//! resulting cookies and storage with [`BrowserSession::state`], closes the
//! session and continues over plain HTTP with [`crate::http::HttpClient`].
//!
//! Secrets (filled values, cookie and storage values, the helper token) are
//! held in [`SecretString`]s, never logged and never shown by `Debug`.
//!
//! Use [`BrowserClient::with_session`]: it always closes the session, even
//! when a step fails. Tests run offline against a scripted
//! [`BrowserBackend`] (`testkit::ScriptedBrowser`) instead of the helper.
//!
//! ```no_run
//! # async fn demo(browser: &solde_connector_sdk::browser::BrowserClient)
//! #     -> Result<(), solde_connector_sdk::ConnectorError> {
//! use std::time::Duration;
//! use solde_connector_sdk::browser::{Action, SessionOptions};
//!
//! let cookie = browser
//!     .with_session(&SessionOptions::default(), |s| {
//!         Box::pin(async move {
//!             s.run(&[
//!                 Action::goto("https://bank.invalid/login"),
//!                 Action::fill("#user", "jane.doe"),
//!                 Action::fill_secret("#password", "not-a-real-password"),
//!                 Action::click("button[type=submit]"),
//!                 Action::wait_for("#dashboard", Duration::from_secs(15)),
//!             ])
//!             .await?;
//!             Ok(s.state().await?.cookie_header("bank.invalid"))
//!         })
//!     })
//!     .await?;
//! # let _ = cookie;
//! # Ok(())
//! # }
//! ```
//!
//! Success results: every successful action returns a JSON object; `text`,
//! `attribute` and `screenshot` put their output (element text, attribute
//! value, base64 PNG) under `"value"`. The first failing action stops the
//! batch with `{"error": "timeout" | "not_found" | "navigation_failed", "op_index": n}`.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::connector::ConnectorError;
use crate::http::{MAX_BODY_BYTES, Url, network_error, redact_path, tls_config};
use crate::secret::SecretString;

/// Default port of the helper (`SOLDE_BROWSER_HELPER_PORT`).
pub const DEFAULT_PORT: u16 = 8765;

/// One browser action. `Debug` never shows filled values.
pub enum Action {
    Goto {
        url: String,
    },
    Fill {
        selector: String,
        value: SecretString,
        secret: bool,
    },
    Click {
        selector: String,
    },
    WaitForSelector {
        selector: String,
        timeout_ms: u64,
    },
    WaitForUrl {
        url_contains: String,
        timeout_ms: u64,
    },
    Text {
        selector: String,
    },
    Attribute {
        selector: String,
        name: String,
    },
    Screenshot {
        selector: String,
    },
}

impl Action {
    pub fn goto(url: &str) -> Self {
        Self::Goto {
            url: url.to_owned(),
        }
    }

    /// Fills a non-secret value (it may appear in helper logs).
    pub fn fill(selector: &str, value: &str) -> Self {
        Self::Fill {
            selector: selector.to_owned(),
            value: SecretString::new(value.to_owned()),
            secret: false,
        }
    }

    /// Fills a secret value; the helper must never log it.
    pub fn fill_secret(selector: &str, value: &str) -> Self {
        Self::Fill {
            selector: selector.to_owned(),
            value: SecretString::new(value.to_owned()),
            secret: true,
        }
    }

    pub fn click(selector: &str) -> Self {
        Self::Click {
            selector: selector.to_owned(),
        }
    }

    pub fn wait_for(selector: &str, timeout: Duration) -> Self {
        Self::WaitForSelector {
            selector: selector.to_owned(),
            timeout_ms: duration_ms(timeout),
        }
    }

    pub fn wait_for_url(url_contains: &str, timeout: Duration) -> Self {
        Self::WaitForUrl {
            url_contains: url_contains.to_owned(),
            timeout_ms: duration_ms(timeout),
        }
    }

    pub fn text(selector: &str) -> Self {
        Self::Text {
            selector: selector.to_owned(),
        }
    }

    pub fn attribute(selector: &str, name: &str) -> Self {
        Self::Attribute {
            selector: selector.to_owned(),
            name: name.to_owned(),
        }
    }

    pub fn screenshot(selector: &str) -> Self {
        Self::Screenshot {
            selector: selector.to_owned(),
        }
    }

    /// The protocol `op` name (`"goto"`, `"fill"`...).
    pub fn op(&self) -> &'static str {
        match self {
            Self::Goto { .. } => "goto",
            Self::Fill { .. } => "fill",
            Self::Click { .. } => "click",
            Self::WaitForSelector { .. } | Self::WaitForUrl { .. } => "wait_for",
            Self::Text { .. } => "text",
            Self::Attribute { .. } => "attribute",
            Self::Screenshot { .. } => "screenshot",
        }
    }

    /// The CSS selector, for actions that have one.
    pub fn selector(&self) -> Option<&str> {
        match self {
            Self::Fill { selector, .. }
            | Self::Click { selector }
            | Self::WaitForSelector { selector, .. }
            | Self::Text { selector }
            | Self::Attribute { selector, .. }
            | Self::Screenshot { selector } => Some(selector),
            Self::Goto { .. } | Self::WaitForUrl { .. } => None,
        }
    }

    /// Whether this is a fill of a secret value.
    pub fn is_secret(&self) -> bool {
        matches!(self, Self::Fill { secret: true, .. })
    }

    /// The value of a fill. Never log it.
    pub fn expose_value(&self) -> Option<&str> {
        match self {
            Self::Fill { value, .. } => Some(value.expose()),
            _ => None,
        }
    }

    fn wire(&self) -> WireAction<'_> {
        match self {
            Self::Goto { url } => WireAction::Goto { url },
            Self::Fill {
                selector,
                value,
                secret,
            } => WireAction::Fill {
                selector,
                value: value.expose(),
                secret: *secret,
            },
            Self::Click { selector } => WireAction::Click { selector },
            Self::WaitForSelector {
                selector,
                timeout_ms,
            } => WireAction::WaitFor {
                selector: Some(selector),
                url_contains: None,
                timeout_ms: *timeout_ms,
            },
            Self::WaitForUrl {
                url_contains,
                timeout_ms,
            } => WireAction::WaitFor {
                selector: None,
                url_contains: Some(url_contains),
                timeout_ms: *timeout_ms,
            },
            Self::Text { selector } => WireAction::Text { selector },
            Self::Attribute { selector, name } => WireAction::Attribute { selector, name },
            Self::Screenshot { selector } => WireAction::Screenshot { selector },
        }
    }
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fill {
                selector, secret, ..
            } => f
                .debug_struct("Fill")
                .field("selector", selector)
                .field("secret", secret)
                .finish_non_exhaustive(),
            other => f.write_str(other.op()),
        }
    }
}

fn duration_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum WireAction<'a> {
    Goto {
        url: &'a str,
    },
    Fill {
        selector: &'a str,
        value: &'a str,
        secret: bool,
    },
    Click {
        selector: &'a str,
    },
    WaitFor {
        #[serde(skip_serializing_if = "Option::is_none")]
        selector: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        url_contains: Option<&'a str>,
        timeout_ms: u64,
    },
    Text {
        selector: &'a str,
    },
    Attribute {
        selector: &'a str,
        name: &'a str,
    },
    Screenshot {
        selector: &'a str,
    },
}

/// Options of a new browser session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionOptions {
    pub user_agent: Option<String>,
    pub locale: String,
    pub viewport: Viewport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            user_agent: None,
            locale: "fr-FR".to_owned(),
            viewport: Viewport {
                width: 1280,
                height: 800,
            },
        }
    }
}

/// Why an action failed (the batch stopped there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionFailure {
    Timeout,
    NotFound,
    NavigationFailed,
    /// An error code this client does not know.
    Other(String),
}

/// Errors from the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserError {
    /// Missing or wrong token (`401`).
    Unauthorized,
    /// The session does not exist or expired (`404`).
    SessionNotFound,
    /// Action number `op_index` (0-based) failed; earlier ones succeeded.
    Action {
        op_index: usize,
        failure: ActionFailure,
    },
    /// The helper is unreachable.
    Unavailable(String),
    /// Unexpected status or payload.
    Protocol(String),
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("browser helper rejected the token"),
            Self::SessionNotFound => f.write_str("browser session not found"),
            Self::Action { op_index, failure } => {
                write!(f, "browser action {op_index} failed: {failure:?}")
            }
            Self::Unavailable(m) => write!(f, "browser helper unavailable: {m}"),
            Self::Protocol(m) => write!(f, "browser helper protocol error: {m}"),
        }
    }
}

impl std::error::Error for BrowserError {}

impl From<BrowserError> for ConnectorError {
    fn from(err: BrowserError) -> Self {
        match err {
            BrowserError::Unavailable(m) => ConnectorError::Network(format!("browser helper: {m}")),
            other => ConnectorError::broken(other.to_string()),
        }
    }
}

/// A cookie read from the browser. The value is a secret.
pub struct Cookie {
    pub name: String,
    pub value: SecretString,
    pub domain: String,
    pub path: String,
    /// Unix seconds; `None` for session cookies.
    pub expires: Option<i64>,
    pub http_only: bool,
    pub secure: bool,
}

/// Browser state after login. Holds secrets: no `Debug`, no `Clone`.
pub struct BrowserState {
    pub url: String,
    pub cookies: Vec<Cookie>,
    /// `origin -> key -> value`.
    pub local_storage: BTreeMap<String, BTreeMap<String, SecretString>>,
}

impl BrowserState {
    /// `name=value; name2=value2` for the cookies whose domain matches `host`
    /// (exact or parent domain). Wrap the result in a secret.
    pub fn cookie_header(&self, host: &str) -> SecretString {
        let header = self
            .cookies
            .iter()
            .filter(|c| {
                let domain = c.domain.trim_start_matches('.');
                host == domain || host.ends_with(&format!(".{domain}"))
            })
            .map(|c| format!("{}={}", c.name, c.value.expose()))
            .collect::<Vec<_>>()
            .join("; ");
        SecretString::new(header)
    }
}

#[derive(Deserialize)]
struct WireState {
    url: String,
    #[serde(default)]
    cookies: Vec<WireCookie>,
    #[serde(default)]
    local_storage: BTreeMap<String, BTreeMap<String, SecretString>>,
}

#[derive(Deserialize)]
struct WireCookie {
    name: String,
    value: SecretString,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    expires: Option<serde_json::Number>,
    #[serde(default)]
    http_only: bool,
    #[serde(default)]
    secure: bool,
}

/// Where browser sessions run: the real helper ([`BrowserClient::new`]) or a
/// scripted fake in tests (`testkit::ScriptedBrowser`).
#[async_trait]
pub trait BrowserBackend: Send + Sync {
    async fn health(&self) -> Result<(), BrowserError>;
    /// Opens a session; returns its id.
    async fn create_session(&self, options: &SessionOptions) -> Result<String, BrowserError>;
    async fn run(
        &self,
        session_id: &str,
        actions: &[Action],
    ) -> Result<Vec<Option<String>>, BrowserError>;
    async fn state(&self, session_id: &str) -> Result<BrowserState, BrowserError>;
    async fn close(&self, session_id: &str) -> Result<(), BrowserError>;
}

/// A boxed future borrowing a [`BrowserSession`], for
/// [`BrowserClient::with_session`].
pub type SessionFuture<'s, T> =
    Pin<Box<dyn Future<Output = Result<T, ConnectorError>> + Send + 's>>;

/// Client of the browser helper. Cheap to clone.
#[derive(Clone)]
pub struct BrowserClient {
    backend: Arc<dyn BrowserBackend>,
}

impl BrowserClient {
    /// The real helper at `base`, e.g. `http://127.0.0.1:8765/`. Plain HTTP
    /// is accepted only on loopback (`127.0.0.1`, `::1`, `localhost`);
    /// anything else must be HTTPS.
    pub fn new(base: Url, token: SecretString) -> Result<Self, BrowserError> {
        Ok(Self::from_backend(Arc::new(HelperBackend::new(
            base, token,
        )?)))
    }

    /// Helper on `127.0.0.1`, port from `SOLDE_BROWSER_HELPER_PORT` (default
    /// 8765), token from `SOLDE_BROWSER_HELPER_TOKEN`.
    pub fn from_env() -> Result<Self, BrowserError> {
        let port = match std::env::var("SOLDE_BROWSER_HELPER_PORT") {
            Ok(p) => p.parse::<u16>().map_err(|_| {
                BrowserError::Protocol("invalid SOLDE_BROWSER_HELPER_PORT".to_owned())
            })?,
            Err(_) => DEFAULT_PORT,
        };
        let token =
            std::env::var("SOLDE_BROWSER_HELPER_TOKEN").map_err(|_| BrowserError::Unauthorized)?;
        let base = Url::parse(&format!("http://127.0.0.1:{port}/"))
            .map_err(|_| BrowserError::Protocol("invalid helper URL".to_owned()))?;
        Self::new(base, SecretString::new(token))
    }

    /// Any [`BrowserBackend`], e.g. a scripted one in tests.
    pub fn from_backend(backend: Arc<dyn BrowserBackend>) -> Self {
        Self { backend }
    }

    /// `GET /healthz`.
    pub async fn health(&self) -> Result<(), BrowserError> {
        self.backend.health().await
    }

    /// `POST /v1/sessions`: opens a fresh, throwaway browser profile. Prefer
    /// [`BrowserClient::with_session`], which always closes it.
    pub async fn create_session(
        &self,
        options: &SessionOptions,
    ) -> Result<BrowserSession, BrowserError> {
        let id = self.backend.create_session(options).await?;
        Ok(BrowserSession {
            backend: self.backend.clone(),
            id,
        })
    }

    /// Opens a session, runs `f` with it, then always closes it, even if `f`
    /// failed. `f`'s error wins over a close error.
    pub async fn with_session<T, F>(
        &self,
        options: &SessionOptions,
        f: F,
    ) -> Result<T, ConnectorError>
    where
        F: for<'s> FnOnce(&'s BrowserSession) -> SessionFuture<'s, T>,
    {
        let session = self.create_session(options).await?;
        let result = f(&session).await;
        let closed = session.close().await;
        match (result, closed) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(e)) => Err(e.into()),
            (Err(e), _) => Err(e),
        }
    }
}

/// An open browser session. An abandoned session expires after 10 minutes of
/// inactivity; [`BrowserClient::with_session`] closes it for you.
pub struct BrowserSession {
    backend: Arc<dyn BrowserBackend>,
    id: String,
}

impl BrowserSession {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// `POST /v1/sessions/{id}/actions`. Returns one output per action
    /// (`Some` for `text`, `attribute` and `screenshot`).
    pub async fn run(&self, actions: &[Action]) -> Result<Vec<Option<String>>, BrowserError> {
        self.backend.run(&self.id, actions).await
    }

    /// `GET /v1/sessions/{id}/state`: current URL, cookies and local storage.
    pub async fn state(&self) -> Result<BrowserState, BrowserError> {
        self.backend.state(&self.id).await
    }

    /// `DELETE /v1/sessions/{id}`: closes the browser and wipes the profile.
    pub async fn close(self) -> Result<(), BrowserError> {
        self.backend.close(&self.id).await
    }
}

/// The browser-helper HTTP protocol (v1).
struct HelperBackend {
    base: Url,
    token: SecretString,
    http: reqwest::Client,
}

impl HelperBackend {
    fn new(base: Url, token: SecretString) -> Result<Self, BrowserError> {
        let loopback = matches!(
            base.host_str(),
            Some("127.0.0.1" | "[::1]" | "::1" | "localhost")
        );
        match base.scheme() {
            "https" => {}
            "http" if loopback => {}
            _ => {
                return Err(BrowserError::Protocol(
                    "the helper URL must be https, or http on loopback".to_owned(),
                ));
            }
        }
        let tls =
            tls_config().map_err(|_| BrowserError::Protocol("cannot configure TLS".to_owned()))?;
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| BrowserError::Protocol("cannot build the HTTP client".to_owned()))?;
        Ok(Self { base, token, http })
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<(u16, Zeroizing<Vec<u8>>), BrowserError> {
        let shown = redact_path(path);
        tracing::debug!(method = %method, path = %shown, "browser helper request");
        let url = self
            .base
            .join(path)
            .map_err(|_| BrowserError::Protocol("invalid helper URL".to_owned()))?;
        let bearer = Zeroizing::new(format!("Bearer {}", self.token.expose()));
        let mut auth = reqwest::header::HeaderValue::from_str(&bearer)
            .map_err(|_| BrowserError::Protocol("invalid token".to_owned()))?;
        auth.set_sensitive(true);
        let mut req = self
            .http
            .request(method, url)
            .header(reqwest::header::AUTHORIZATION, auth);
        if let Some(body) = body {
            req = req
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| BrowserError::Unavailable(network_error(&e).to_string()))?;
        let status = resp.status().as_u16();
        let bytes = crate::http::read_capped(resp).await.map_err(|e| match e {
            ConnectorError::Network(m) => BrowserError::Unavailable(m),
            other => BrowserError::Protocol(format!(
                "{other} (limit {} MiB)",
                MAX_BODY_BYTES / (1024 * 1024)
            )),
        })?;
        tracing::debug!(path = %shown, status, "browser helper response");
        match status {
            401 => Err(BrowserError::Unauthorized),
            404 => Err(BrowserError::SessionNotFound),
            _ => Ok((status, bytes)),
        }
    }
}

#[async_trait]
impl BrowserBackend for HelperBackend {
    async fn health(&self) -> Result<(), BrowserError> {
        match self.call(reqwest::Method::GET, "healthz", None).await? {
            (200, _) => Ok(()),
            (s, _) => Err(BrowserError::Protocol(format!("unexpected status {s}"))),
        }
    }

    async fn create_session(&self, options: &SessionOptions) -> Result<String, BrowserError> {
        let body = serde_json::to_vec(options)
            .map_err(|_| BrowserError::Protocol("cannot encode options".to_owned()))?;
        let (status, bytes) = self
            .call(reqwest::Method::POST, "v1/sessions", Some(body))
            .await?;
        if status != 201 {
            return Err(BrowserError::Protocol(format!(
                "unexpected status {status}"
            )));
        }
        #[derive(Deserialize)]
        struct Created {
            session_id: String,
        }
        let created: Created = serde_json::from_slice(&bytes)
            .map_err(|_| BrowserError::Protocol("invalid session response".to_owned()))?;
        if created.session_id.is_empty()
            || !created
                .session_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(BrowserError::Protocol("invalid session id".to_owned()));
        }
        Ok(created.session_id)
    }

    async fn run(
        &self,
        session_id: &str,
        actions: &[Action],
    ) -> Result<Vec<Option<String>>, BrowserError> {
        #[derive(Serialize)]
        struct Batch<'a> {
            actions: Vec<WireAction<'a>>,
        }
        let ops: Vec<&str> = actions.iter().map(Action::op).collect();
        tracing::debug!(ops = ?ops, "browser actions");
        let batch = Batch {
            actions: actions.iter().map(Action::wire).collect(),
        };
        // The encoded batch contains filled values: zeroize our copy (reqwest
        // keeps its own, which cannot be wiped).
        let body = Zeroizing::new(
            serde_json::to_vec(&batch)
                .map_err(|_| BrowserError::Protocol("cannot encode actions".to_owned()))?,
        );
        let path = format!("v1/sessions/{session_id}/actions");
        let mut body = body;
        // Hand the buffer itself to reqwest (no extra copy); the emptied
        // wrapper is wiped, reqwest's own buffers cannot be.
        let (status, bytes) = self
            .call(
                reqwest::Method::POST,
                &path,
                Some(std::mem::take(&mut *body)),
            )
            .await?;
        if status != 200 {
            return Err(BrowserError::Protocol(format!(
                "unexpected status {status}"
            )));
        }
        #[derive(Deserialize)]
        struct Results {
            results: Vec<Value>,
        }
        let results: Results = serde_json::from_slice(&bytes)
            .map_err(|_| BrowserError::Protocol("invalid actions response".to_owned()))?;
        let mut outputs = Vec::with_capacity(results.results.len());
        for (i, result) in results.results.into_iter().enumerate() {
            if let Some(code) = result.get("error").and_then(Value::as_str) {
                let op_index = result
                    .get("op_index")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(i);
                let failure = match code {
                    "timeout" => ActionFailure::Timeout,
                    "not_found" => ActionFailure::NotFound,
                    "navigation_failed" => ActionFailure::NavigationFailed,
                    other => ActionFailure::Other(other.chars().take(64).collect()),
                };
                return Err(BrowserError::Action { op_index, failure });
            }
            outputs.push(
                result
                    .get("value")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            );
        }
        if outputs.len() != actions.len() {
            return Err(BrowserError::Protocol(format!(
                "expected {} results, got {}",
                actions.len(),
                outputs.len()
            )));
        }
        Ok(outputs)
    }

    async fn state(&self, session_id: &str) -> Result<BrowserState, BrowserError> {
        let path = format!("v1/sessions/{session_id}/state");
        let (status, bytes) = self.call(reqwest::Method::GET, &path, None).await?;
        if status != 200 {
            return Err(BrowserError::Protocol(format!(
                "unexpected status {status}"
            )));
        }
        let wire: WireState = serde_json::from_slice(&bytes)
            .map_err(|_| BrowserError::Protocol("invalid state response".to_owned()))?;
        Ok(BrowserState {
            url: wire.url,
            cookies: wire
                .cookies
                .into_iter()
                .map(|c| Cookie {
                    name: c.name,
                    value: c.value,
                    domain: c.domain,
                    path: c.path,
                    // Playwright reports -1 (or a fractional second) for these.
                    expires: c
                        .expires
                        .and_then(|n| n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)))
                        .filter(|&s| s >= 0),
                    http_only: c.http_only,
                    secure: c.secure,
                })
                .collect(),
            local_storage: wire.local_storage,
        })
    }

    async fn close(&self, session_id: &str) -> Result<(), BrowserError> {
        let path = format!("v1/sessions/{session_id}");
        match self.call(reqwest::Method::DELETE, &path, None).await? {
            (204 | 200, _) => Ok(()),
            (s, _) => Err(BrowserError::Protocol(format!("unexpected status {s}"))),
        }
    }
}

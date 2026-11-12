// SPDX-License-Identifier: AGPL-3.0-or-later
//! Two-factor / strong customer authentication (SCA), enforced by type.
//!
//! Hard rule: scheduled, background and MCP-triggered syncs run in
//! [`InteractionMode::Unattended`] and must never trigger an OTP, SMS or app
//! notification. Only a sync started by the connection owner runs in
//! [`InteractionMode::Interactive`].
//!
//! A connector never sees the app's [`Interaction`] directly: `login` gets a
//! [`LoginContext`], built by the app with [`LoginContext::interactive`] or
//! [`LoginContext::unattended`]. In Unattended mode it holds no interaction
//! and NO credentials, so a connector cannot start a password login (the
//! usual OTP trigger), [`LoginContext::challenge`] can only refuse, and
//! [`LoginContext::send`] refuses requests marked
//! [`HttpRequest::triggers_sca`] before they leave the machine. The plain
//! [`HttpClient::send`] refuses marked requests outright, so marking one
//! forces it through the context.

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;

use std::collections::BTreeMap;

use crate::connector::ConnectorError;
use crate::http::{HttpClient, HttpRequest, HttpResponse, Url, redact_path};
use crate::secret::{CredentialSet, SecretString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InteractionMode {
    /// A user clicked "sync now": challenges (SMS, app validation) are allowed.
    Interactive,
    /// Scheduled or MCP-triggered sync: reuse the session only. Never trigger an OTP.
    Unattended,
}

/// Something the connector needs the user to do. Labels are short English
/// strings; the app may translate known ones. More kinds may be added later:
/// match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Challenge {
    /// "Code received by SMS". Answer: [`ChallengeResponse::Code`].
    Otp { label: String },
    /// "Confirm in your bank app"; resolves when the user taps "done".
    /// Answer: [`ChallengeResponse::Validated`].
    AppValidation { label: String },
    /// Pick one of `options`. Answer: [`ChallengeResponse::Chosen`].
    Choice { label: String, options: Vec<String> },
    /// Open `url` (a bank or aggregator consent page) in the user's browser;
    /// resolves when the bank redirects back to the app's callback. Answer:
    /// [`ChallengeResponse::Redirected`]. Build `url` with the callback URL
    /// and `state` of [`LoginContext::redirect_target`], then check the
    /// answer with [`LoginContext::verify_callback`].
    Redirect { label: String, url: String },
}

/// The user's answer. Holds secrets (a code, a callback URL carrying an
/// authorization code), so it has no `Debug` or `Clone`.
#[non_exhaustive]
pub enum ChallengeResponse {
    /// Answer to [`Challenge::Otp`].
    Code(SecretString),
    /// Answer to [`Challenge::AppValidation`]: the user says they confirmed.
    Validated,
    /// Answer to [`Challenge::Choice`]: index into `options`.
    Chosen(usize),
    /// Answer to [`Challenge::Redirect`]: the full callback URL the bank
    /// redirected to (query included).
    Redirected { callback_url: SecretString },
}

/// Why the app could not get an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InteractionError {
    /// The user dismissed the challenge.
    Cancelled,
    /// The user did not answer in time.
    Timeout,
}

impl fmt::Display for InteractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "the user cancelled the challenge",
            Self::Timeout => "the user did not answer the challenge in time",
        })
    }
}

impl std::error::Error for InteractionError {}

/// Implemented by the app (web modal, desktop dialog) to show a challenge to
/// the connection owner and wait for the answer.
#[async_trait]
pub trait Interaction: Send + Sync {
    async fn challenge(&self, c: Challenge) -> Result<ChallengeResponse, InteractionError>;
}

/// What `login` gets: the mode, and in Interactive mode a way to ask the user.
pub struct LoginContext<'a> {
    mode: InteractionMode,
    io: Option<&'a dyn Interaction>,
    credentials: Option<CredentialSet>,
    redirect: Option<RedirectTarget>,
    timeout: Duration,
    challenge_attempts: AtomicUsize,
    refused_requests: AtomicUsize,
}

impl<'a> LoginContext<'a> {
    /// How long the user has to answer by default.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

    /// A sync the connection owner started: challenges go to `io`, and the
    /// connection's credentials are available (wiped when the context drops).
    pub fn interactive(io: &'a dyn Interaction, credentials: CredentialSet) -> Self {
        Self {
            mode: InteractionMode::Interactive,
            io: Some(io),
            credentials: Some(credentials),
            redirect: None,
            timeout: Self::DEFAULT_TIMEOUT,
            challenge_attempts: AtomicUsize::new(0),
            refused_requests: AtomicUsize::new(0),
        }
    }

    /// A scheduled, background or MCP sync: no interaction and no
    /// credentials exist; only the saved session can be reused.
    pub fn unattended() -> LoginContext<'static> {
        LoginContext {
            mode: InteractionMode::Unattended,
            io: None,
            credentials: None,
            redirect: None,
            timeout: Self::DEFAULT_TIMEOUT,
            challenge_attempts: AtomicUsize::new(0),
            refused_requests: AtomicUsize::new(0),
        }
    }

    /// Where a [`Challenge::Redirect`] must come back to, for this job. Set by
    /// the app on an interactive context; ignored in Unattended mode (no
    /// redirect can happen without the user).
    pub fn with_redirect_target(mut self, target: RedirectTarget) -> Self {
        if self.mode == InteractionMode::Interactive {
            self.redirect = Some(target);
        }
        self
    }

    /// The app's callback URL and single-use `state` for a redirect flow.
    /// Always `None` in Unattended mode.
    pub fn redirect_target(&self) -> Option<&RedirectTarget> {
        self.redirect.as_ref()
    }

    /// Checks the URL a [`ChallengeResponse::Redirected`] returned: same
    /// scheme, host, port and path as the redirect target's callback, and a
    /// `state` parameter equal to the target's (compared in constant time).
    /// Returns the callback's query parameters (e.g. `code`).
    pub fn verify_callback(
        &self,
        callback_url: &SecretString,
    ) -> Result<CallbackParams, ConnectorError> {
        let Some(target) = &self.redirect else {
            return Err(ConnectorError::CallbackRejected {
                reason: "no redirect target for this login".to_owned(),
            });
        };
        target.verify(callback_url)
    }

    /// How long the user has to answer a challenge.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn mode(&self) -> InteractionMode {
        self.mode
    }

    pub fn is_unattended(&self) -> bool {
        self.mode == InteractionMode::Unattended
    }

    /// The connection's credentials: `Some` in Interactive mode only.
    pub fn credentials(&self) -> Option<&CredentialSet> {
        self.credentials.as_ref()
    }

    /// Like [`LoginContext::credentials`], but `None` is
    /// [`ConnectorError::ScaRequired`]: without the user, a fresh login is
    /// not possible.
    pub fn credentials_required(&self) -> Result<&CredentialSet, ConnectorError> {
        self.credentials().ok_or(ConnectorError::ScaRequired)
    }

    /// Asks the user. Unattended: [`ConnectorError::ScaRequired`], always.
    /// Interactive: the answer, or [`ConnectorError::ChallengeCancelled`] /
    /// [`ConnectorError::ChallengeTimeout`].
    pub async fn challenge(&self, c: Challenge) -> Result<ChallengeResponse, ConnectorError> {
        self.challenge_attempts.fetch_add(1, Ordering::SeqCst);
        let Some(io) = self.io else {
            return Err(ConnectorError::ScaRequired);
        };
        match tokio::time::timeout(self.timeout, io.challenge(c)).await {
            Ok(answer) => answer.map_err(ConnectorError::from),
            Err(_) => Err(ConnectorError::ChallengeTimeout),
        }
    }

    /// Sends `req` with `http`. A request marked
    /// [`HttpRequest::triggers_sca`] is refused in Unattended mode with
    /// [`ConnectorError::ScaRequired`], and nothing is sent.
    pub async fn send(
        &self,
        http: &HttpClient,
        req: HttpRequest,
    ) -> Result<HttpResponse, ConnectorError> {
        if req.is_sca_trigger() && self.is_unattended() {
            self.refused_requests.fetch_add(1, Ordering::SeqCst);
            tracing::warn!(
                method = %req.method(),
                path = %redact_path(req.url().path()),
                "refused an SCA-triggering request in unattended mode"
            );
            return Err(ConnectorError::ScaRequired);
        }
        http.send_checked(req).await
    }

    /// How many times the connector called [`LoginContext::challenge`].
    pub fn challenge_attempts(&self) -> usize {
        self.challenge_attempts.load(Ordering::SeqCst)
    }

    /// How many SCA-triggering requests were refused (Unattended only).
    pub fn refused_requests(&self) -> usize {
        self.refused_requests.load(Ordering::SeqCst)
    }
}

/// Where a redirect-based consent must come back: the app's callback URL and
/// a single-use, unguessable `state`, created by the app for one sync job.
#[non_exhaustive]
pub struct RedirectTarget {
    pub callback_url: Url,
    pub state: SecretString,
}

impl RedirectTarget {
    pub fn new(callback_url: Url, state: SecretString) -> Self {
        Self {
            callback_url,
            state,
        }
    }

    fn verify(&self, returned: &SecretString) -> Result<CallbackParams, ConnectorError> {
        let reject = |reason: &str| ConnectorError::CallbackRejected {
            reason: reason.to_owned(),
        };
        let url = Url::parse(returned.expose()).map_err(|_| reject("not a URL"))?;
        let expected = &self.callback_url;
        if url.scheme() != expected.scheme()
            || url.host_str() != expected.host_str()
            || url.port_or_known_default() != expected.port_or_known_default()
            || url.path() != expected.path()
        {
            return Err(reject("not the expected callback URL"));
        }
        let mut params = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            if params.contains_key(key.as_ref()) {
                return Err(reject("duplicate query parameter"));
            }
            params.insert(key.into_owned(), SecretString::new(value.into_owned()));
        }
        match params.get("state") {
            Some(state) if constant_time_eq(state.expose(), self.state.expose()) => {}
            Some(_) => return Err(reject("state mismatch")),
            None => return Err(reject("missing state")),
        }
        Ok(CallbackParams { params })
    }
}

/// Query parameters of a verified callback. Values may be secrets (an
/// authorization code): no `Debug`, no `Clone`.
pub struct CallbackParams {
    params: BTreeMap<String, SecretString>,
}

impl CallbackParams {
    /// Reads one parameter (e.g. `"code"`). Never log the result.
    pub fn expose(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(SecretString::expose)
    }

    /// Like [`CallbackParams::expose`], but a missing parameter is a
    /// [`ConnectorError::CallbackRejected`].
    pub fn expose_required(&self, name: &str) -> Result<&str, ConnectorError> {
        self.expose(name)
            .ok_or_else(|| ConnectorError::CallbackRejected {
                reason: format!("missing `{name}` parameter"),
            })
    }
}

/// Compares in time independent of where the inputs first differ (the length
/// is not secret).
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{FixtureSet, Url};

    struct Never;

    #[async_trait]
    impl Interaction for Never {
        async fn challenge(&self, _c: Challenge) -> Result<ChallengeResponse, InteractionError> {
            std::future::pending().await
        }
    }

    struct Answers(Result<(), InteractionError>);

    #[async_trait]
    impl Interaction for Answers {
        async fn challenge(&self, _c: Challenge) -> Result<ChallengeResponse, InteractionError> {
            self.0.map(|()| ChallengeResponse::Validated)
        }
    }

    fn otp() -> Challenge {
        Challenge::Otp {
            label: "Code received by SMS".to_owned(),
        }
    }

    #[tokio::test]
    async fn unattended_always_refuses() {
        let ctx = LoginContext::unattended();
        assert!(matches!(
            ctx.challenge(otp()).await,
            Err(ConnectorError::ScaRequired)
        ));
        assert_eq!(ctx.challenge_attempts(), 1);
        let io = Answers(Ok(()));
        let ctx = LoginContext::interactive(&io, CredentialSet::new());
        assert!(matches!(
            ctx.challenge(otp()).await,
            Ok(ChallengeResponse::Validated)
        ));
    }

    #[test]
    fn credentials_only_when_interactive() {
        let io = Answers(Ok(()));
        let ctx = LoginContext::interactive(&io, CredentialSet::new().with("password", "x"));
        assert_eq!(
            ctx.credentials_required().unwrap().expose("password"),
            Some("x")
        );
        let ctx = LoginContext::unattended();
        assert!(ctx.credentials().is_none());
        assert!(matches!(
            ctx.credentials_required(),
            Err(ConnectorError::ScaRequired)
        ));
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }

    #[test]
    fn unattended_has_no_redirect_target() {
        let target = RedirectTarget::new(
            Url::parse("https://app.invalid/cb").unwrap(),
            SecretString::new("s"),
        );
        let ctx = LoginContext::unattended().with_redirect_target(target);
        assert!(ctx.redirect_target().is_none());
        assert!(matches!(
            ctx.verify_callback(&SecretString::new("https://app.invalid/cb?state=s")),
            Err(ConnectorError::CallbackRejected { .. })
        ));
    }

    #[tokio::test]
    async fn challenge_errors() {
        let io = Never;
        let ctx = LoginContext::interactive(&io, CredentialSet::new())
            .with_timeout(Duration::from_millis(10));
        assert!(matches!(
            ctx.challenge(otp()).await,
            Err(ConnectorError::ChallengeTimeout)
        ));
        let io = Answers(Err(InteractionError::Cancelled));
        let ctx = LoginContext::interactive(&io, CredentialSet::new());
        assert!(matches!(
            ctx.challenge(otp()).await,
            Err(ConnectorError::ChallengeCancelled)
        ));
    }

    #[tokio::test]
    async fn sca_requests_need_an_interactive_context() {
        let set = FixtureSet::from_static(&[(
            "1.json",
            r#"{"request":{"method":"POST","path":"/login"},"response":{"status":200}}"#,
        )])
        .unwrap();
        let http = HttpClient::replay(set).unwrap();
        let login =
            || HttpRequest::post(Url::parse("https://b.invalid/login").unwrap()).triggers_sca();

        // Outside a context: refused, whatever the mode.
        assert!(matches!(
            http.send(login()).await,
            Err(ConnectorError::Broken { .. })
        ));
        // Unattended: refused before anything is sent.
        let ctx = LoginContext::unattended();
        assert!(matches!(
            ctx.send(&http, login()).await,
            Err(ConnectorError::ScaRequired)
        ));
        assert_eq!(ctx.refused_requests(), 1);
        assert_eq!(http.request_count(), 0);
        // Interactive: sent.
        let io = Answers(Ok(()));
        let ctx = LoginContext::interactive(&io, CredentialSet::new());
        assert_eq!(ctx.send(&http, login()).await.unwrap().status(), 200);
        assert_eq!(http.request_count(), 1);
    }
}

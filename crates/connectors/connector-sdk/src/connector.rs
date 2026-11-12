// SPDX-License-Identifier: AGPL-3.0-or-later
//! The connector trait, its manifest and its error type.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use chrono::NaiveDate;

use crate::browser::BrowserClient;
use crate::clock::Clock;
use crate::dto::{RemoteAccount, RemoteAccountId, RemotePosition, RemoteTransaction};
use crate::http::HttpClient;
use crate::interaction::{InteractionError, LoginContext};
use crate::secret::SessionBlob;

/// Static description of a connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorManifest {
    /// Stable, unique id (`"fake-bank"`, `"banque-postale"`).
    pub id: &'static str,
    /// Name shown to users (a brand name, not translated).
    pub display_name: &'static str,
    pub capabilities: Capabilities,
    /// What the user must type to connect, in display order.
    pub credential_fields: Vec<CredentialField>,
    pub stability: Stability,
}

/// What the connector can fetch. Start from [`Capabilities::none`] and add
/// what you support (more capabilities may be added later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Capabilities {
    pub accounts: bool,
    pub transactions: bool,
    pub positions: bool,
    pub quotes: bool,
}

impl Capabilities {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn with_accounts(mut self) -> Self {
        self.accounts = true;
        self
    }

    pub fn with_transactions(mut self) -> Self {
        self.transactions = true;
        self
    }

    pub fn with_positions(mut self) -> Self {
        self.positions = true;
        self
    }

    pub fn with_quotes(mut self) -> Self {
        self.quotes = true;
        self
    }
}

/// What the app hands a connector when it builds one instance for one bank
/// connection: its own [`HttpClient`] (with that connection's rate limiter),
/// a [`BrowserClient`] if the browser helper is available, and the [`Clock`].
#[derive(Clone)]
#[non_exhaustive]
pub struct ConnectorEnv {
    pub http: HttpClient,
    pub browser: Option<BrowserClient>,
    pub clock: Clock,
}

impl ConnectorEnv {
    pub fn new(http: HttpClient, clock: Clock) -> Self {
        Self {
            http,
            browser: None,
            clock,
        }
    }

    pub fn with_browser(mut self, browser: BrowserClient) -> Self {
        self.browser = Some(browser);
        self
    }
}

/// One input of the "add a bank" form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialField {
    /// Key in the [`CredentialSet`].
    pub name: &'static str,
    /// English label; the app translates known names.
    pub label: &'static str,
    /// Masked input; never echoed back.
    pub secret: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stability {
    /// Not implemented yet: must not be offered to users.
    Planned,
    /// Works, but the bank may break it at any time. Every new direct
    /// connector starts here.
    Experimental,
    Stable,
}

/// Connector failures. `Debug`/`Display` never contain secrets or personal
/// data: keep `detail` and network messages generic. More variants may be
/// added: match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectorError {
    /// Wrong credentials.
    AuthFailed,
    /// The bank refused the code the user typed.
    OtpRejected,
    /// The user dismissed the challenge.
    ChallengeCancelled,
    /// The user did not answer the challenge in time.
    ChallengeTimeout,
    /// A redirect came back to the wrong URL, without the expected `state`,
    /// or without a required parameter. `reason` holds no secret.
    CallbackRejected {
        reason: String,
    },
    SessionExpired,
    /// Strong auth needed; in Unattended mode: stop, send nothing.
    ScaRequired,
    RateLimited {
        retry_after: Option<Duration>,
    },
    /// The bank changed something; no personal data in `detail`.
    Broken {
        detail: String,
    },
    Unsupported,
    Network(String),
}

impl ConnectorError {
    /// Shorthand for [`ConnectorError::Broken`].
    pub fn broken(detail: impl Into<String>) -> Self {
        Self::Broken {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthFailed => f.write_str("authentication failed"),
            Self::OtpRejected => f.write_str("the bank rejected the code"),
            Self::ChallengeCancelled => f.write_str("the user cancelled the challenge"),
            Self::ChallengeTimeout => f.write_str("the user did not answer the challenge in time"),
            Self::CallbackRejected { reason } => write!(f, "redirect callback rejected: {reason}"),
            Self::SessionExpired => f.write_str("session expired"),
            Self::ScaRequired => f.write_str("strong customer authentication required"),
            Self::RateLimited {
                retry_after: Some(d),
            } => write!(f, "rate limited (retry after {}s)", d.as_secs()),
            Self::RateLimited { retry_after: None } => f.write_str("rate limited"),
            Self::Broken { detail } => write!(f, "connector broken: {detail}"),
            Self::Unsupported => f.write_str("not supported by this connector"),
            Self::Network(msg) => write!(f, "network error: {msg}"),
        }
    }
}

impl std::error::Error for ConnectorError {}

impl From<InteractionError> for ConnectorError {
    fn from(err: InteractionError) -> Self {
        match err {
            InteractionError::Cancelled => Self::ChallengeCancelled,
            InteractionError::Timeout => Self::ChallengeTimeout,
        }
    }
}

#[async_trait]
pub trait BankConnector: Send + Sync {
    /// id, capabilities {accounts, transactions, positions, quotes}, credential fields, stability.
    fn manifest(&self) -> ConnectorManifest;

    /// `prev` is the last saved session, if any. The credentials are in
    /// `ctx` (Interactive mode only: [`LoginContext::credentials`] is `None`
    /// when unattended, so a password login cannot even start).
    ///
    /// When `ctx` is Unattended, only reuse `prev`. If strong auth would be
    /// needed, return [`ConnectorError::ScaRequired`] before sending anything
    /// to the bank that could trigger an OTP. Ask the user with
    /// [`LoginContext::challenge`], and send OTP-triggering requests (marked
    /// [`crate::http::HttpRequest::triggers_sca`]) with [`LoginContext::send`].
    async fn login(
        &self,
        ctx: &LoginContext<'_>,
        prev: Option<SessionBlob>,
    ) -> Result<SessionBlob, ConnectorError>;

    async fn accounts(&self, s: &SessionBlob) -> Result<Vec<RemoteAccount>, ConnectorError>;

    /// `since` is always the start of a full booking day.
    async fn transactions(
        &self,
        s: &SessionBlob,
        account: &RemoteAccountId,
        since: NaiveDate,
    ) -> Result<Vec<RemoteTransaction>, ConnectorError>;

    /// Default: Err(Unsupported).
    async fn positions(
        &self,
        _s: &SessionBlob,
        _account: &RemoteAccountId,
    ) -> Result<Vec<RemotePosition>, ConnectorError> {
        Err(ConnectorError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_display_without_data() {
        assert_eq!(
            ConnectorError::RateLimited {
                retry_after: Some(Duration::from_secs(30))
            }
            .to_string(),
            "rate limited (retry after 30s)"
        );
        assert_eq!(
            ConnectorError::from(InteractionError::Timeout),
            ConnectorError::ChallengeTimeout
        );
        assert_eq!(
            ConnectorError::from(InteractionError::Cancelled),
            ConnectorError::ChallengeCancelled
        );
    }

    // `BankConnector` must stay dyn-compatible: the registry boxes connectors.
    #[allow(dead_code)]
    fn assert_dyn_compatible(_: &dyn BankConnector) {}
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Connector SDK: everything a bank connector needs, and the only way it may
//! reach the network.
//!
//! - The contract: [`BankConnector`], [`LoginContext`], [`Interaction`], the
//!   DTOs ([`RemoteAccount`], [`RemoteTransaction`], [`RemotePosition`]) and
//!   [`ConnectorError`].
//! - Secret holders: [`CredentialSet`], [`SessionBlob`], [`SecretString`]. They
//!   implement neither `Debug`, `Display`, `Serialize` nor `Clone`, and are
//!   zeroized on drop.
//! - [`http::HttpClient`]: the mandatory HTTP client (redacted logs, rate
//!   limiting hook, fixture record/replay with scrubbing).
//! - [`browser::BrowserClient`]: client for the browser-helper service.
//! - [`ids::synthesize_remote_ids`]: stable ids for banks that give none.
//! - [`Clock`]: today's date at the bank (fixed in tests).
//! - [`ConnectorEnv`]: what the app hands a connector for one connection.
//! - [`scrub::Scrubber`]: anonymizes recorded fixtures.
//! - `testkit` (feature `testkit`): the shared test harness every connector uses.
//!
//! Connector crates may only depend on this crate plus the third-party crates
//! listed in `crates/connectors/allowed-deps.toml`. See
//! `docs/connectors/writing-a-connector.md`.
//!
//! Secret types cannot be printed or copied:
//!
//! ```compile_fail
//! fn leak(c: solde_connector_sdk::CredentialSet) -> String { format!("{c:?}") }
//! ```
//!
//! ```compile_fail
//! fn copy(s: &solde_connector_sdk::SessionBlob) -> solde_connector_sdk::SessionBlob { s.clone() }
//! ```
//!
//! ```compile_fail
//! fn leak(s: solde_connector_sdk::SecretString) -> String { format!("{s}") }
//! ```
//!
//! ```compile_fail
//! fn ser<T: serde::Serialize>(_: &T) {}
//! fn leak(s: &solde_connector_sdk::SessionBlob) { ser(s) }
//! ```

// Bank connector crates are barred from HTTP, WebSocket and socket types by the
// clippy configuration in `crates/connectors/clippy.toml`. The SDK is the one
// place where that I/O is implemented, so the ban is lifted here only.
#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

pub mod browser;
mod clock;
mod connector;
mod dto;
pub mod http;
pub mod ids;
mod interaction;
mod money;
pub mod scrub;
mod secret;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use async_trait::async_trait;
pub use chrono::NaiveDate;
pub use clock::{Clock, Tz};
pub use connector::{
    BankConnector, Capabilities, ConnectorEnv, ConnectorError, ConnectorManifest, CredentialField,
    Stability,
};
pub use dto::{
    AccountKind, RemoteAccount, RemoteAccountId, RemotePosition, RemoteTransaction,
    TransactionStatus,
};
pub use interaction::{
    CallbackParams, Challenge, ChallengeResponse, Interaction, InteractionError, InteractionMode,
    LoginContext, RedirectTarget,
};
pub use money::{Currency, InvalidCurrency, InvalidQuantity, Quantity};
pub use secret::{CredentialSet, SecretString, SessionBlob};

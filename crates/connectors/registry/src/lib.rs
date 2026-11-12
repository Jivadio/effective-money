// SPDX-License-Identifier: AGPL-3.0-or-later
//! Registry of the available connectors.
//!
//! Each [`ConnectorEntry`] carries the connector's manifest and a factory: the
//! app builds one instance per bank connection, with that connection's own
//! [`ConnectorEnv`] (HTTP client and rate limiter, optional browser, clock).
//!
//! Only connectors that actually implement [`BankConnector`] are registered.
//! The Enable Banking, Trade Republic, Banque Populaire and La Banque Postale
//! crates are placeholders for now: their `bank-*` features exist so builds
//! stay stable, but they register nothing until their connector lands.
//! `bank-fake` (the demo bank, which always replays its embedded fixtures) is
//! off by default.

// The registry is not a bank connector: it builds the demo bank's replay
// client, which bank crates themselves may not do (crates/connectors/clippy.toml).
#![allow(clippy::disallowed_methods)]

use solde_connector_sdk::{BankConnector, ConnectorEnv, ConnectorError, ConnectorManifest};

/// Builds one connector instance for one bank connection.
pub type ConnectorFactory = fn(ConnectorEnv) -> Result<Box<dyn BankConnector>, ConnectorError>;

/// A registered connector: what it is, and how to build it.
pub struct ConnectorEntry {
    pub manifest: ConnectorManifest,
    pub build: ConnectorFactory,
}

/// Every enabled connector.
pub fn entries() -> Vec<ConnectorEntry> {
    // Placeholders add nothing until their connector lands.
    let entries: [Option<ConnectorEntry>; 1] = [
        #[cfg(feature = "bank-fake")]
        Some(ConnectorEntry {
            manifest: solde_connector_fake_bank::manifest(),
            // The demo bank has no server: it ignores `env` and replays its
            // embedded fixtures with a clock fixed at their date.
            build: |_env| Ok(Box::new(fake_bank_demo()?)),
        }),
        #[cfg(not(feature = "bank-fake"))]
        None,
    ];
    entries.into_iter().flatten().collect()
}

/// The demo Fake Bank: embedded fixtures (repeating), clock fixed at their date.
#[cfg(feature = "bank-fake")]
pub fn fake_bank_demo() -> Result<solde_connector_fake_bank::FakeBank, ConnectorError> {
    use solde_connector_fake_bank::{DEMO_TODAY, FakeBank, demo_fixtures};
    use solde_connector_sdk::http::HttpClient;
    use solde_connector_sdk::{Clock, NaiveDate};
    let (y, m, d) = DEMO_TODAY;
    let today =
        NaiveDate::from_ymd_opt(y, m, d).ok_or_else(|| ConnectorError::broken("bad demo date"))?;
    let http = HttpClient::replay(demo_fixtures()?)?;
    FakeBank::new(ConnectorEnv::new(http, Clock::fixed(today)))
}

/// The entry with this manifest id, if registered.
pub fn by_id(id: &str) -> Option<ConnectorEntry> {
    entries().into_iter().find(|e| e.manifest.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solde_connector_sdk::Stability;

    #[test]
    fn ids_are_unique_and_nothing_is_planned() {
        let entries = entries();
        let mut ids: Vec<_> = entries.iter().map(|e| e.manifest.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), entries.len());
        assert!(
            entries
                .iter()
                .all(|e| e.manifest.stability != Stability::Planned)
        );
        assert!(by_id("banque-postale").is_none());
    }

    #[cfg(feature = "bank-fake")]
    #[test]
    fn fake_bank_builds() {
        use solde_connector_sdk::http::{HttpClient, HttpConfig};
        use solde_connector_sdk::{Clock, Tz};
        let entry = by_id("fake-bank").unwrap();
        let env = ConnectorEnv::new(
            HttpClient::new(HttpConfig::live()).unwrap(),
            Clock::system(Tz::Europe__Paris),
        );
        let connector = (entry.build)(env).unwrap();
        assert_eq!(connector.manifest(), entry.manifest);
    }

    #[cfg(feature = "bank-fake")]
    #[test]
    fn demo_syncs_offline_repeatedly() {
        use solde_connector_sdk::testkit::{
            Scripted, ScriptedInteraction, block_on, full_sync, interactive,
        };
        use solde_connector_sdk::{CredentialSet, NaiveDate};
        let bank = fake_bank_demo().unwrap();
        for _ in 0..2 {
            let io = ScriptedInteraction::new(vec![Scripted::Code("123456".to_owned())]);
            let creds = CredentialSet::new()
                .with("username", "jane.doe")
                .with("password", "not-a-real-password");
            let since = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
            let run = block_on(full_sync(&bank, &interactive(&io, creds), None, since)).unwrap();
            assert_eq!(run.accounts.len(), 3);
        }
    }

    #[cfg(not(feature = "bank-fake"))]
    #[test]
    fn fake_bank_is_off_by_default() {
        assert!(by_id("fake-bank").is_none());
    }
}

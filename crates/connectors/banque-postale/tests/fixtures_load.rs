// SPDX-License-Identifier: AGPL-3.0-or-later
//! Guards the shape of the fixture set: every scenario directory, and every
//! overlay the connector tests will use, must load through
//! [`FixtureSet`]. A malformed fixture fails here rather than in the connector
//! tests.
//!
//! Repeating a request key is legal: [`FixtureSet`] queues the responses and
//! serves them in file-name order, which is how a retry or a poll is modelled
//! (see `server-error/`). These tests therefore prove loading, not uniqueness.

use solde_connector_sdk::http::FixtureSet;

fn fixtures_dir(name: &str) -> String {
    format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn every_scenario_loads_on_its_own() {
    for name in [
        "login-ok",
        "challenge",
        "refused",
        "expired",
        "auth-failed",
        "server-error",
        "pending-day1",
        "pending-day2",
    ] {
        FixtureSet::from_dir(fixtures_dir(name)).unwrap_or_else(|err| {
            panic!("scenario {name} does not load: {err}");
        });
    }
}

#[test]
fn overlays_load_on_top_of_login_ok() {
    for overlay in ["challenge", "refused", "server-error"] {
        FixtureSet::from_dirs(&[fixtures_dir("login-ok"), fixtures_dir(overlay)])
            .unwrap_or_else(|err| panic!("overlay {overlay} does not load: {err}"));
    }
    FixtureSet::from_dirs(&[
        fixtures_dir("login-ok"),
        fixtures_dir("pending-day1"),
        fixtures_dir("pending-day2"),
    ])
    .expect("the pending overlays do not load on top of login-ok");
}

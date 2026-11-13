// SPDX-License-Identifier: AGPL-3.0-or-later
//! Guards the shape of the fixture set: every scenario directory, and every
//! overlay the connector tests will use, must load through [`FixtureSet`] and
//! carry the request keys it is meant to answer.
//!
//! A malformed fixture fails here rather than in the connector tests. A
//! scenario emptied by accident would also load, so each one is checked
//! against its expected number of distinct request keys.
//!
//! Repeating a request key is legal: [`FixtureSet`] queues the responses and
//! serves them in file-name order, which is how a retry or a poll is modelled
//! (`server-error/` queues two `502`s, `challenge/` a `200` then a `302`).
//! These tests prove loading and key coverage, not behaviour.

use solde_connector_sdk::http::FixtureSet;

fn fixtures_dir(name: &str) -> String {
    format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Every scenario directory and the number of distinct request keys it holds.
/// The count is the guard against a scenario silently emptied (the loader
/// accepts an empty directory).
const SCENARIOS: &[(&str, usize)] = &[
    ("login-ok", 10),
    ("challenge", 6),
    ("refused", 6),
    ("relogin", 5),
    ("expired", 1),
    ("auth-failed", 3),
    ("auth-param-refused", 2),
    ("entry-blocked", 1),
    ("server-error", 1),
    ("savings-fallback", 2),
    ("empty-history", 1),
    ("cardless-synthesis", 1),
    ("unknown-product", 1),
    ("pending-day1", 1),
    ("pending-day2", 1),
];

#[test]
fn every_scenario_loads_on_its_own() {
    for (name, expected_keys) in SCENARIOS {
        let set = FixtureSet::from_dir(fixtures_dir(name))
            .unwrap_or_else(|err| panic!("scenario {name} does not load: {err}"));
        assert!(!set.is_empty(), "{name} answers no request at all");
        assert_eq!(
            set.len(),
            *expected_keys,
            "{name} should answer {expected_keys} distinct request keys"
        );
    }
}

#[test]
fn overlays_load_on_top_of_login_ok() {
    // Each overlay replaces the keys it defines in `login-ok/`, so the merged
    // set keeps the base keys it does not override.
    for overlay in [
        "challenge",
        "refused",
        "relogin",
        "server-error",
        "savings-fallback",
        "empty-history",
        "cardless-synthesis",
        "unknown-product",
        "entry-blocked",
        "auth-param-refused",
    ] {
        let set = FixtureSet::from_dirs(&[fixtures_dir("login-ok"), fixtures_dir(overlay)])
            .unwrap_or_else(|err| panic!("overlay {overlay} does not load: {err}"));
        assert!(!set.is_empty(), "overlay {overlay} produced an empty set");
    }
}

#[test]
fn pending_overlays_are_loaded_one_at_a_time() {
    // `pending-day1` and `pending-day2` answer the same request key, and a
    // later layer replaces the earlier one: loading both together would only
    // ever exercise `day2`. Pending → booked is two syncs, so each overlay is
    // loaded on its own.
    for overlay in ["pending-day1", "pending-day2"] {
        let set = FixtureSet::from_dirs(&[fixtures_dir("login-ok"), fixtures_dir(overlay)])
            .unwrap_or_else(|err| panic!("pending overlay {overlay} does not load: {err}"));
        assert_eq!(
            set.len(),
            10,
            "{overlay} should answer the ten login-ok keys"
        );
    }
}

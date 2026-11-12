// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fake Bank against its replayed fixtures, using the SDK testkit. Copy this
//! file when you write a connector: every connector needs these checks.

use std::path::PathBuf;

use solde_connector_fake_bank::{FakeBank, manifest};
use solde_connector_sdk::http::{HttpClient, Method, RequestSummary};
use solde_connector_sdk::testkit::{
    CANARY, Scripted, ScriptedInteraction, SyncRun, assert_ids_stable, assert_no_canary,
    assert_pending_became_booked, assert_sends_nothing, assert_twins_preserved, assert_unique_ids,
    block_on, canary_credentials, capture_logs, check_session_holds_no_credentials, full_sync,
    interactive, live_credentials, live_env, login_unattended, replay_env,
};
use solde_connector_sdk::{
    BankConnector, Challenge, ConnectorError, CredentialSet, InteractionError, NaiveDate,
    RemoteAccountId, SessionBlob, TransactionStatus, Tz,
};

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

/// The day the `day1` fixtures were "recorded".
fn day1() -> NaiveDate {
    date(2026, 1, 15)
}

/// A Fake Bank replaying `scenarios` (overlaid in order) on `today`, and its
/// HTTP client to inspect the requests.
fn fake_bank(scenarios: &[&str], today: NaiveDate) -> (FakeBank, HttpClient) {
    let dirs: Vec<PathBuf> = scenarios
        .iter()
        .map(|s| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures")
                .join(s)
        })
        .collect();
    let (env, http) = replay_env(&dirs, today);
    (FakeBank::new(env).expect("connector"), http)
}

fn creds() -> CredentialSet {
    CredentialSet::new()
        .with("username", "jane.doe")
        .with("password", "not-a-real-password")
}

fn since() -> NaiveDate {
    date(2026, 1, 1)
}

/// A saved session, valid until `valid_until`.
fn session(valid_until: &str) -> SessionBlob {
    SessionBlob::new(
        format!(r#"{{"token":"tok-CANARY-7f3a9c","valid_until":"{valid_until}"}}"#).into_bytes(),
    )
}

fn interactive_sync(scenarios: &[&str], today: NaiveDate) -> SyncRun {
    let (bank, _) = fake_bank(scenarios, today);
    let io = ScriptedInteraction::new(vec![Scripted::Code("123456".to_owned())]);
    let run = block_on(full_sync(&bank, &interactive(&io, creds()), None, since()))
        .expect("sync succeeds");
    assert_eq!(
        io.challenges(),
        [Challenge::Otp {
            label: "Code received by SMS".to_owned()
        }]
    );
    run
}

fn req(method: Method, path: &str) -> RequestSummary {
    RequestSummary {
        method,
        path: path.to_owned(),
    }
}

#[test]
fn full_replay() {
    let run = interactive_sync(&["day1"], day1());
    assert_eq!(run.accounts.len(), 3);
    assert_eq!(run.accounts[0].balance_minor, Some(152_347));

    let checking = &run.transactions[&RemoteAccountId::new("CHK-001")];
    // Today's (2026-01-15) id-less transaction is dropped; the one before
    // `since` is not returned.
    assert_eq!(checking.len(), 6);
    assert!(checking.iter().all(|t| t.booking_date >= since()));
    assert!(!checking.iter().any(|t| t.label == "CARD PAYMENT BAKERY"));
    let electricity = checking
        .iter()
        .find(|t| t.remote_id.as_deref() == Some("tx-1001"))
        .unwrap();
    assert_eq!(electricity.amount_minor, -4290);
    assert_eq!(
        checking
            .iter()
            .filter(|t| t.status == TransactionStatus::Pending)
            .count(),
        1
    );
    // Ids only need to be unique within one account.
    for transactions in run.transactions.values() {
        assert_unique_ids(transactions);
    }

    let positions = &run.positions[&RemoteAccountId::new("INV-001")];
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].quantity.to_string(), "12.5");
    assert_eq!(positions[0].market_value_minor, Some(123_000));
}

#[test]
fn twins_preserved() {
    let run = interactive_sync(&["day1"], day1());
    // Twins and ids are checked per account.
    let checking = &run.transactions[&RemoteAccountId::new("CHK-001")];
    assert_twins_preserved(checking, 1);
}

#[test]
fn ids_stable_across_runs() {
    let a = interactive_sync(&["day1"], day1());
    let b = interactive_sync(&["day1"], day1());
    assert_ids_stable(&a.all_transactions(), &b.all_transactions());
    // Ids of complete days survive the next day's sync too.
    let day2 = interactive_sync(&["day1", "day2"], date(2026, 1, 16));
    let ids = |r: &SyncRun| -> Vec<String> {
        r.all_transactions()
            .into_iter()
            .filter_map(|t| t.remote_id)
            .collect()
    };
    let later = ids(&day2);
    for id in ids(&a) {
        assert!(later.contains(&id), "{id} disappeared on day 2");
    }
}

#[test]
fn pending_becomes_booked() {
    let day1 = interactive_sync(&["day1"], day1());
    let day2 = interactive_sync(&["day1", "day2"], date(2026, 1, 16));
    assert_pending_became_booked(&day1.all_transactions(), &day2.all_transactions());
}

#[test]
fn unattended_without_session_sends_nothing() {
    let (bank, http) = fake_bank(&["day1"], day1());
    // `login_unattended` itself checks that no request was sent.
    let report = block_on(login_unattended(&bank, &http, None));
    assert_eq!(report.result.err(), Some(ConnectorError::ScaRequired));
}

#[test]
fn unattended_with_a_known_expired_session_sends_nothing() {
    let (bank, http) = fake_bank(&["day1"], day1());
    let report = block_on(login_unattended(&bank, &http, Some(session("2026-01-10"))));
    assert_sends_nothing(&report);
    assert_eq!(report.result.err(), Some(ConnectorError::ScaRequired));
}

#[test]
fn unattended_with_a_revoked_session_sends_no_login() {
    let (bank, http) = fake_bank(&["day1", "expired"], day1());
    let report = block_on(login_unattended(&bank, &http, Some(session("2026-04-15"))));
    assert_eq!(report.result.err(), Some(ConnectorError::ScaRequired));
    // Only the session check, which never triggers an OTP.
    assert_eq!(report.requests, [req(Method::Get, "/api/session")]);
}

#[test]
fn unattended_reuses_a_valid_session() {
    let (bank, http) = fake_bank(&["day1"], day1());
    let report = block_on(login_unattended(&bank, &http, Some(session("2026-04-15"))));
    let blob = report.result.ok().unwrap();
    assert_eq!(blob.expose(), session("2026-04-15").expose());
    assert_eq!(report.requests, [req(Method::Get, "/api/session")]);
}

#[test]
fn unreadable_session_is_treated_as_expired() {
    let garbage = || SessionBlob::new(b"not json".to_vec());
    // Unattended: ScaRequired, nothing sent.
    let (bank, http) = fake_bank(&["day1"], day1());
    let report = block_on(login_unattended(&bank, &http, Some(garbage())));
    assert_eq!(report.result.err(), Some(ConnectorError::ScaRequired));
    // Interactive: straight to the full login, no session check.
    let (bank, http) = fake_bank(&["day1"], day1());
    let io = ScriptedInteraction::new(vec![Scripted::Code("123456".to_owned())]);
    let r = block_on(bank.login(&interactive(&io, creds()), Some(garbage())));
    assert!(r.is_ok());
    assert_eq!(io.count(), 1);
    assert_eq!(
        http.requests(),
        [
            req(Method::Post, "/api/login"),
            req(Method::Post, "/api/login/otp")
        ]
    );
}

#[test]
fn interactive_login_flow() {
    let (bank, http) = fake_bank(&["day1", "expired"], day1());
    let io = ScriptedInteraction::new(vec![Scripted::Code("123456".to_owned())]);
    let blob = block_on(bank.login(&interactive(&io, creds()), Some(session("2026-04-15"))))
        .ok()
        .unwrap();
    let saved = std::str::from_utf8(blob.expose()).unwrap();
    assert!(saved.contains("tok-CANARY-7f3a9c"), "{saved}");
    // The SID cookie is kept, the unrelated one is not.
    assert!(saved.contains("SID=CANARY-7f3a9c-sid"), "{saved}");
    assert!(!saved.contains("lang="), "{saved}");
    assert_eq!(io.count(), 1);
    assert_eq!(
        http.requests(),
        [
            req(Method::Get, "/api/session"),
            req(Method::Post, "/api/login"),
            req(Method::Post, "/api/login/otp")
        ]
    );
}

#[test]
fn login_errors() {
    // Wrong password: no challenge at all.
    let (bank, _) = fake_bank(&["day1", "auth-failed"], day1());
    let io = ScriptedInteraction::new(vec![]);
    let r = block_on(bank.login(&interactive(&io, creds()), None));
    assert_eq!(r.err(), Some(ConnectorError::AuthFailed));
    assert_eq!(io.count(), 0);

    // The user cancels, or does not answer in time.
    for (failure, expected) in [
        (
            InteractionError::Cancelled,
            ConnectorError::ChallengeCancelled,
        ),
        (InteractionError::Timeout, ConnectorError::ChallengeTimeout),
    ] {
        let (bank, _) = fake_bank(&["day1"], day1());
        let io = ScriptedInteraction::new(vec![Scripted::Fail(failure)]);
        let r = block_on(bank.login(&interactive(&io, creds()), None));
        assert_eq!(r.err(), Some(expected));
    }

    // The bank rejects the code.
    let (bank, _) = fake_bank(&["day1", "otp-rejected"], day1());
    let io = ScriptedInteraction::new(vec![Scripted::Code("000000".to_owned())]);
    let r = block_on(bank.login(&interactive(&io, creds()), None));
    assert_eq!(r.err(), Some(ConnectorError::OtpRejected));
}

#[test]
fn unusable_session_during_sync() {
    let (bank, _) = fake_bank(&["day1"], day1());
    let garbage = SessionBlob::new(b"not json".to_vec());
    let r = block_on(bank.accounts(&garbage));
    assert_eq!(r.err(), Some(ConnectorError::SessionExpired));
}

#[test]
fn bank_data_edge_cases() {
    let account = RemoteAccountId::new("CHK-001");
    let s = session("2026-04-15");

    let (bank, _) = fake_bank(&["day1", "empty-page"], day1());
    assert_eq!(
        block_on(bank.transactions(&s, &account, since())),
        Ok(vec![])
    );

    let (bank, _) = fake_bank(&["day1", "looping"], day1());
    assert_eq!(
        block_on(bank.transactions(&s, &account, since())),
        Err(ConnectorError::broken("pagination goes backwards"))
    );

    let (bank, _) = fake_bank(&["day1", "bad-currency"], day1());
    assert_eq!(
        block_on(bank.accounts(&s)),
        Err(ConnectorError::broken(
            "unknown currency code `ZZZ` (not in the SDK's ISO 4217 table, connector-sdk/src/money.rs)"
        ))
    );
}

#[test]
fn session_holds_no_credentials() {
    let (bank, _) = fake_bank(&["day1"], day1());
    block_on(check_session_holds_no_credentials(&bank));
}

#[test]
fn no_secret_in_logs() {
    // The fixtures' token and cookie contain the canary too.
    let (bank, _) = fake_bank(&["day1"], day1());
    let creds = canary_credentials(&manifest());
    let io = ScriptedInteraction::new(vec![Scripted::Code(format!("otp-{CANARY}"))]);
    let (result, logs) =
        capture_logs(|| block_on(full_sync(&bank, &interactive(&io, creds), None, since())));
    assert!(result.is_ok());
    assert!(logs.contains("http request"), "{logs}");
    assert_no_canary(&logs, "logs");

    // Errors do not carry secrets either.
    let (bank, _) = fake_bank(&["day1", "auth-failed"], day1());
    let creds = canary_credentials(&manifest());
    let (result, logs) = capture_logs(|| block_on(bank.login(&interactive(&io, creds), None)));
    let err = result.err().unwrap();
    assert_no_canary(&format!("{err} {err:?}"), "the error");
    assert_no_canary(&logs, "logs");
}

/// Run against a real account, on your machine only:
/// `SOLDE_LIVE_USERNAME=... SOLDE_LIVE_PASSWORD=... cargo test -p <crate> -- --ignored live`.
/// For Fake Bank this cannot succeed (`fake-bank.invalid` does not exist);
/// it shows the shape to copy. Never commit credentials or raw recordings.
#[test]
#[ignore = "needs a real account and network"]
fn live_against_your_own_account() {
    let Some(creds) = live_credentials(&manifest(), "SOLDE_LIVE") else {
        return;
    };
    // To record fixtures, use `testkit::record_env(dir, scrubber, zone)` with
    // `dir` OUTSIDE the repository; scrub, review, then copy files in.
    let bank = FakeBank::new(live_env(Tz::Europe__Paris)).unwrap();
    let io = ScriptedInteraction::new(vec![]);
    let r = block_on(bank.login(&interactive(&io, creds), None));
    // Report only the outcome, never data.
    println!(
        "login: {}",
        r.map(|_| "ok".to_owned()).unwrap_or_else(|e| e.to_string())
    );
}

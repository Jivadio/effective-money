// SPDX-License-Identifier: AGPL-3.0-or-later
//! `HttpClient` in Live and Record modes against a local mock server.

// The SDK's own tests exercise what bank crates may not use (building
// clients, local servers, temp files); the bans target bank crates only.
#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

mod common;

use axum::Json;
use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use serde_json::{Value, json};
use solde_connector_sdk::ConnectorError;
use solde_connector_sdk::http::{FixtureSet, HttpClient, HttpConfig, HttpRequest, Url};
use solde_connector_sdk::testkit::{CANARY, assert_no_canary, block_on, capture_logs};

use common::spawn;

fn bank() -> Router {
    Router::new()
        .route(
            "/api/login",
            post(|headers: HeaderMap, body: String| async move {
                // The secret reached the server, in the header and the body.
                assert!(
                    headers["authorization"]
                        .to_str()
                        .expect("ASCII header")
                        .contains(CANARY)
                );
                assert!(body.contains(CANARY));
                (
                    [("set-cookie", "SESSION=s3cr3t-cookie; Path=/; HttpOnly")],
                    Json(json!({"accessToken": "tok-abc", "holder": {"firstName": "Alice"}})),
                )
            }),
        )
        .route(
            "/api/accounts",
            get(|| async {
                Json(json!({"accounts": [
                    {"id": "A1", "accountNumber": "12345678901",
                     "iban": "FR00TEST000000000000000001A", "label": "MRS A EXAMPLE", "balance": 12345}
                ]}))
            }),
        )
        .route(
            "/api/accounts/12345678901/transactions",
            get(|| async { Json(json!({"transactions": []})) }),
        )
        .route(
            "/oauth/authorize",
            get(|| async {
                (
                    StatusCode::FOUND,
                    [("location", "https://bank.invalid/callback?code=CANARY-7f3a9c-code&lang=fr")],
                )
            }),
        )
        .route(
            "/api/slow-down",
            get(|| async { (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "7")]) }),
        )
        .route(
            "/api/huge",
            get(|| async { vec![b'x'; solde_connector_sdk::http::MAX_BODY_BYTES + 1] }),
        )
}

#[test]
fn live_requests_and_redacted_logs() {
    let (_, logs) = capture_logs(|| {
        block_on(async {
            let base = spawn(bank()).await;
            let http = HttpClient::new(HttpConfig::live().with_plain_http()).unwrap();

            let login = HttpRequest::post(base.join(&format!("api/login?otp={CANARY}")).unwrap())
                .bearer(CANARY)
                .json(&json!({"password": CANARY}))
                .unwrap();
            let resp = http.send(login).await.unwrap();
            assert_eq!(resp.status(), 200);
            assert_eq!(resp.json::<Value>().unwrap()["accessToken"], "tok-abc");
            assert!(resp.header("Set-Cookie").unwrap().starts_with("SESSION="));

            let txs = base.join("api/accounts/12345678901/transactions").unwrap();
            assert_eq!(
                http.send(HttpRequest::get(txs)).await.unwrap().status(),
                200
            );

            let err = http
                .send(HttpRequest::get(base.join("api/slow-down").unwrap()))
                .await
                .unwrap_err();
            assert_eq!(
                err,
                ConnectorError::RateLimited {
                    retry_after: Some(std::time::Duration::from_secs(7))
                }
            );

            let huge = http
                .send(HttpRequest::get(base.join("api/huge").unwrap()))
                .await
                .unwrap_err();
            assert_eq!(huge, ConnectorError::broken("response body too large"));
        })
    });
    assert!(logs.contains("path=/api/login"), "{logs}");
    assert!(
        logs.contains("path=/api/accounts/{id}/transactions"),
        "{logs}"
    );
    assert!(logs.contains("status=200"), "{logs}");
    assert_no_canary(&logs, "logs");
    for leaked in ["otp=", "tok-abc", "12345678901", "s3cr3t-cookie"] {
        assert!(!logs.contains(leaked), "{leaked} leaked into logs:\n{logs}");
    }
}

#[test]
fn record_scrubs_then_replays_offline() {
    let dir = std::env::temp_dir().join(format!("solde-sdk-record-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    block_on(async {
        let base = spawn(bank()).await;
        let http = HttpClient::new(HttpConfig::record(&dir).unwrap().with_plain_http()).unwrap();
        let login = HttpRequest::post(base.join("api/login").unwrap())
            .bearer(CANARY)
            .json(&json!({"password": CANARY}))
            .unwrap();
        http.send(login).await.unwrap();
        let live = http
            .send(HttpRequest::get(base.join("api/accounts").unwrap()))
            .await
            .unwrap();
        assert_eq!(
            live.json::<Value>().unwrap()["accounts"][0]["iban"],
            "FR00TEST000000000000000001A"
        );
        // An id from the previous body in the path, the canary in the query.
        let txs = base
            .join(&format!(
                "api/accounts/12345678901/transactions?otp={CANARY}&from=2026-01-01"
            ))
            .unwrap();
        http.send(HttpRequest::get(txs)).await.unwrap();
        let redirect = http
            .send(HttpRequest::get(base.join("oauth/authorize").unwrap()))
            .await
            .unwrap();
        assert_eq!(redirect.status(), 302);
    });

    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "0000-post-api-login.json",
            "0001-get-api-accounts.json",
            "0002-get-api-accounts-id00000001-transactions.json",
            "0003-get-oauth-authorize.json"
        ]
    );
    let all: String = names
        .iter()
        .map(|n| std::fs::read_to_string(dir.join(n)).unwrap())
        .collect();
    assert_no_canary(&all, "the recorded fixtures");
    for leaked in [
        "tok-abc",
        "Alice",
        "s3cr3t-cookie",
        "FR00TEST",
        "MRS A EXAMPLE",
        "12345678901",
        "code=CANARY",
    ] {
        assert!(
            !all.contains(leaked),
            "{leaked} leaked into fixtures:\n{all}"
        );
    }

    // Replay the recording offline: scrubbed values come back, consistently.
    block_on(async {
        let replay = HttpClient::replay(FixtureSet::from_dir(&dir).unwrap()).unwrap();
        let url = |p: &str| Url::parse(&format!("https://bank.invalid{p}")).unwrap();
        let body: Value = replay
            .send(HttpRequest::get(url("/api/accounts")))
            .await
            .unwrap()
            .json()
            .unwrap();
        let account = &body["accounts"][0];
        assert_eq!(account["iban"], "FR7600000000000000000000001");
        assert_eq!(account["accountNumber"], "ID00000001");
        assert_eq!(account["balance"], 12345);
        let txs = replay
            .send(HttpRequest::get(url(
                "/api/accounts/ID00000001/transactions?otp=SCRUBBED&from=2026-01-01",
            )))
            .await
            .unwrap();
        assert_eq!(txs.status(), 200);
        let login = replay
            .send(HttpRequest::post(url("/api/login")))
            .await
            .unwrap();
        assert_eq!(login.header("set-cookie"), Some("SESSION=SCRUBBED"));
        assert_eq!(login.json::<Value>().unwrap()["accessToken"], "SCRUBBED");
        let redirect = replay
            .send(HttpRequest::get(url("/oauth/authorize")))
            .await
            .unwrap();
        assert_eq!(
            redirect.header("location"),
            Some("https://bank.invalid/callback?code=SCRUBBED&lang=fr")
        );
    });

    std::fs::remove_dir_all(&dir).unwrap();
}

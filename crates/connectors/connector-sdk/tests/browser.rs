// SPDX-License-Identifier: AGPL-3.0-or-later
//! `BrowserClient` against a mock browser-helper (no real browser).

// The SDK's own tests exercise what bank crates may not use (building
// clients, local servers, temp files); the bans target bank crates only.
#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use serde_json::{Value, json};
use solde_connector_sdk::browser::{
    Action, ActionFailure, BrowserClient, BrowserError, SessionOptions,
};
use solde_connector_sdk::testkit::{
    CANARY, ScriptedBrowser, ScriptedRun, SeenAction, assert_no_canary, block_on, capture_logs,
};
use solde_connector_sdk::{ConnectorError, SecretString};

use common::spawn;

const TOKEN: &str = "helper-token";

#[derive(Clone, Default)]
struct Mock {
    received: Arc<Mutex<Vec<Value>>>,
    closed: Arc<Mutex<usize>>,
}

fn authorized(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {TOKEN}"))
}

fn helper(mock: Mock) -> Router {
    Router::new()
        .route(
            "/healthz",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route(
            "/v1/sessions",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                assert_eq!(body["locale"], "fr-FR");
                assert_eq!(body["viewport"]["width"], 1280);
                (StatusCode::CREATED, Json(json!({"session_id": "s-1"}))).into_response()
            }),
        )
        .route(
            "/v1/sessions/{id}/actions",
            post(
                |State(mock): State<Mock>, headers: HeaderMap, Path(id): Path<String>, Json(body): Json<Value>| async move {
                    if !authorized(&headers) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    if id != "s-1" {
                        return StatusCode::NOT_FOUND.into_response();
                    }
                    mock.received.lock().expect("mock state").push(body.clone());
                    let actions = body["actions"].as_array().expect("an actions array");
                    let mut results = Vec::new();
                    for (i, a) in actions.iter().enumerate() {
                        match a["op"].as_str().expect("an op") {
                            "text" => results.push(json!({"ok": true, "value": "Welcome"})),
                            "click" if a["selector"] == "#missing" => {
                                results.push(json!({"error": "not_found", "op_index": i}));
                                break;
                            }
                            _ => results.push(json!({"ok": true})),
                        }
                    }
                    Json(json!({"results": results})).into_response()
                },
            ),
        )
        .route(
            "/v1/sessions/{id}/state",
            get(|headers: HeaderMap| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                Json(json!({
                    "url": "https://bank.invalid/home",
                    "cookies": [
                        {"name": "SID", "value": "cookie-value", "domain": ".bank.invalid", "path": "/",
                         "expires": -1, "http_only": true, "secure": true},
                        {"name": "OTHER", "value": "x", "domain": "elsewhere.invalid", "path": "/",
                         "expires": 1893456000.5, "http_only": false, "secure": true}
                    ],
                    "local_storage": {"https://bank.invalid": {"token": "ls-token"}}
                }))
                .into_response()
            }),
        )
        .route(
            "/v1/sessions/{id}",
            delete(|State(mock): State<Mock>, headers: HeaderMap| async move {
                if authorized(&headers) {
                    *mock.closed.lock().expect("mock state") += 1;
                    StatusCode::NO_CONTENT
                } else {
                    StatusCode::UNAUTHORIZED
                }
            }),
        )
        .with_state(mock)
}

#[test]
fn full_session_flow() {
    let mock = Mock::default();
    let (_, logs) = capture_logs(|| {
        block_on(async {
            let base = spawn(helper(mock.clone())).await;
            let client = BrowserClient::new(base, SecretString::new(TOKEN)).unwrap();

            client.health().await.unwrap();
            let session = client
                .create_session(&SessionOptions::default())
                .await
                .unwrap();
            assert_eq!(session.id(), "s-1");

            let actions = [
                Action::goto("https://bank.invalid/login"),
                Action::fill("#user", "jane"),
                Action::fill_secret("#password", CANARY),
                Action::click("button[type=submit]"),
                Action::wait_for("#dashboard", Duration::from_secs(15)),
                Action::wait_for_url("/home", Duration::from_secs(15)),
                Action::text(".welcome"),
            ];
            let out = session.run(&actions).await.unwrap();
            assert_eq!(out.len(), 7);
            assert_eq!(out[6].as_deref(), Some("Welcome"));
            assert!(format!("{:?}", actions[2]).contains("secret: true"));
            assert!(!format!("{:?}", actions[2]).contains(CANARY));

            // Wire format sent to the helper.
            let sent = mock.received.lock().unwrap()[0].clone();
            assert_eq!(
                sent["actions"][2],
                json!({"op": "fill", "selector": "#password", "value": CANARY, "secret": true})
            );
            assert_eq!(
                sent["actions"][4],
                json!({"op": "wait_for", "selector": "#dashboard", "timeout_ms": 15000})
            );
            assert_eq!(
                sent["actions"][5],
                json!({"op": "wait_for", "url_contains": "/home", "timeout_ms": 15000})
            );

            // The first failure stops the batch and is reported with its index.
            let err = session
                .run(&[
                    Action::goto("https://bank.invalid/"),
                    Action::click("#missing"),
                    Action::click("#never"),
                ])
                .await
                .unwrap_err();
            assert_eq!(
                err,
                BrowserError::Action {
                    op_index: 1,
                    failure: ActionFailure::NotFound
                }
            );

            let state = session.state().await.unwrap();
            assert_eq!(state.url, "https://bank.invalid/home");
            assert_eq!(state.cookies[0].expires, None);
            assert_eq!(state.cookies[1].expires, Some(1_893_456_000));
            assert_eq!(
                state.cookie_header("www.bank.invalid").expose(),
                "SID=cookie-value"
            );
            assert_eq!(
                state.local_storage["https://bank.invalid"]["token"].expose(),
                "ls-token"
            );
            session.close().await.unwrap();
        })
    });
    assert!(logs.contains("v1/sessions/s-1/actions"), "{logs}");
    assert_no_canary(&logs, "logs");
    for leaked in [TOKEN, "cookie-value", "ls-token"] {
        assert!(!logs.contains(leaked), "{leaked} leaked into logs:\n{logs}");
    }
}

#[tokio::test]
async fn with_session_always_closes() {
    let mock = Mock::default();
    let base = spawn(helper(mock.clone())).await;
    let client = BrowserClient::new(base, SecretString::new(TOKEN)).unwrap();
    let ok = client
        .with_session(&SessionOptions::default(), |s| {
            Box::pin(async move { Ok(s.run(&[Action::text(".welcome")]).await?) })
        })
        .await
        .unwrap();
    assert_eq!(ok, [Some("Welcome".to_owned())]);
    let failed = client
        .with_session(&SessionOptions::default(), |s| {
            Box::pin(async move {
                s.run(&[Action::click("#missing")]).await?;
                Ok(())
            })
        })
        .await;
    assert!(
        matches!(failed, Err(ConnectorError::Broken { .. })),
        "{failed:?}"
    );
    assert_eq!(*mock.closed.lock().unwrap(), 2);
}

#[test]
fn helper_url_must_be_loopback_or_https() {
    let token = || SecretString::new(TOKEN);
    for ok in [
        "http://127.0.0.1:8765/",
        "http://[::1]:8765/",
        "http://localhost:8765/",
        "https://browser-helper.internal/",
    ] {
        assert!(
            BrowserClient::new(ok.parse().unwrap(), token()).is_ok(),
            "{ok}"
        );
    }
    for bad in [
        "http://browser-helper:8765/",
        "http://192.168.1.10:8765/",
        "ftp://127.0.0.1/",
    ] {
        assert!(
            BrowserClient::new(bad.parse().unwrap(), token()).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn scripted_browser_offline() {
    let script = ScriptedBrowser::new(
        vec![ScriptedRun::Ok(vec![None, None, None])],
        "https://bank.invalid/home",
        &[("SID", "scripted-cookie", ".bank.invalid")],
    );
    let client = script.client();
    let cookie = block_on(client.with_session(&SessionOptions::default(), |s| {
        Box::pin(async move {
            s.run(&[
                Action::goto("https://bank.invalid/login"),
                Action::fill_secret("#password", CANARY),
                Action::click("#go"),
            ])
            .await?;
            Ok(s.state().await?.cookie_header("bank.invalid"))
        })
    }))
    .unwrap();
    assert_eq!(cookie.expose(), "SID=scripted-cookie");
    assert_eq!(script.sessions(), (1, 1));
    assert_eq!(
        script.actions()[1],
        SeenAction {
            op: "fill",
            selector: Some("#password".to_owned()),
            secret: true
        }
    );
}

#[tokio::test]
async fn wrong_token_is_unauthorized() {
    let base = spawn(helper(Mock::default())).await;
    let client = BrowserClient::new(base, SecretString::new("wrong")).unwrap();
    let err = client
        .create_session(&SessionOptions::default())
        .await
        .err()
        .unwrap();
    assert_eq!(err, BrowserError::Unauthorized);
}

#[tokio::test]
async fn unreachable_helper_is_a_network_error() {
    // Bind then drop to get a port nobody listens on.
    let port = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let base = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let client = BrowserClient::new(base, SecretString::new(TOKEN)).unwrap();
    let err = client.health().await.unwrap_err();
    assert!(matches!(err, BrowserError::Unavailable(_)), "{err:?}");
    assert!(matches!(
        solde_connector_sdk::ConnectorError::from(err),
        solde_connector_sdk::ConnectorError::Network(_)
    ));
}

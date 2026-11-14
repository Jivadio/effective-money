// SPDX-License-Identifier: AGPL-3.0-or-later
//! Integration test for `GET /healthz`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt as _;
use tower::ServiceExt as _;

async fn app() -> axum::Router {
    let store = solde_services::connect("sqlite::memory:")
        .await
        .expect("in-memory store");
    solde_server::app(
        store,
        solde_api::ApiConfig::default(),
        solde_server::WebAssets::placeholder(),
    )
    .expect("app")
}

#[tokio::test]
async fn healthz_returns_ok_and_version() {
    let response = app()
        .await
        .oneshot(
            Request::get("/healthz")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["version"], solde_server::VERSION);
}

#[tokio::test]
async fn unknown_route_is_404() {
    let response = app()
        .await
        .oneshot(Request::get("/nope").body(Body::empty()).expect("request"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_is_mounted() {
    let response = app()
        .await
        .oneshot(
            Request::get("/api/v1/meta")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

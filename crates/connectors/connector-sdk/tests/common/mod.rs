// SPDX-License-Identifier: AGPL-3.0-or-later
//! A local mock server for the SDK integration tests (127.0.0.1 only).

use axum::Router;
use solde_connector_sdk::http::Url;

/// Serves `app` on an ephemeral loopback port; returns its base URL.
pub async fn spawn(app: Router) -> Url {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a loopback port");
    let addr = listener.local_addr().expect("local address");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("mock server") });
    Url::parse(&format!("http://{addr}/")).expect("mock URL")
}

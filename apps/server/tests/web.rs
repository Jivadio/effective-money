// SPDX-License-Identifier: AGPL-3.0-or-later
//! The web app: content types, cache and security headers, and the
//! boundary with the API (unknown `/api` paths stay JSON 404s).

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt as _;
use solde_server::WebAssets;
use tower::ServiceExt as _;

fn temp_dist(name: &str, with_index: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solde-web-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).expect("temp dir");
    if with_index {
        std::fs::write(
            dir.join("index.html"),
            "<!doctype html><script type=\"module\" src=\"./assets/index-abc123.js\"></script>",
        )
        .expect("index");
    }
    std::fs::write(dir.join("assets/index-abc123.js"), "export {};").expect("js");
    std::fs::write(dir.join("assets/index-abc123.css"), "body{}").expect("css");
    std::fs::write(dir.join("assets/inter-abc123.woff2"), [0_u8, 1, 2]).expect("font");
    dir
}

async fn request(web: WebAssets, method: Method, path: &str) -> (Response, Vec<u8>) {
    let store = solde_services::connect("sqlite::memory:")
        .await
        .expect("in-memory store");
    let app = solde_server::app(store, solde_api::ApiConfig::default(), web).expect("app");
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let (parts, body) = response.into_parts();
    let bytes = body.collect().await.expect("body").to_bytes().to_vec();
    (Response::from_parts(parts, Body::empty()), bytes)
}

fn header(response: &Response, name: header::HeaderName) -> &str {
    response
        .headers()
        .get(&name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn assert_security_headers(response: &Response) {
    let csp = header(response, header::CONTENT_SECURITY_POLICY);
    assert_eq!(csp, solde_server::web::CONTENT_SECURITY_POLICY);
    assert!(csp.contains("script-src 'self'"));
    assert!(csp.contains("frame-ancestors 'none'"));
    assert!(!csp.contains("unsafe-inline"));
    assert!(!csp.contains("unsafe-eval"));
    assert_eq!(header(response, header::X_CONTENT_TYPE_OPTIONS), "nosniff");
    assert_eq!(header(response, header::REFERRER_POLICY), "no-referrer");
}

#[tokio::test]
async fn index_is_html_and_never_cached() {
    let dir = temp_dist("index", true);
    for path in ["/", "/index.html"] {
        let web = WebAssets::from_dir(&dir).expect("dist");
        let (response, body) = request(web, Method::GET, path).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            header(&response, header::CONTENT_TYPE),
            "text/html; charset=utf-8"
        );
        assert_eq!(header(&response, header::CACHE_CONTROL), "no-cache");
        assert_security_headers(&response);
        assert!(String::from_utf8_lossy(&body).contains("index-abc123.js"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn hashed_assets_are_immutable_with_their_content_type() {
    let dir = temp_dist("assets", true);
    for (path, content_type) in [
        ("/assets/index-abc123.js", "text/javascript; charset=utf-8"),
        ("/assets/index-abc123.css", "text/css; charset=utf-8"),
        ("/assets/inter-abc123.woff2", "font/woff2"),
    ] {
        let web = WebAssets::from_dir(&dir).expect("dist");
        let (response, _) = request(web, Method::GET, path).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(header(&response, header::CONTENT_TYPE), content_type);
        assert_eq!(
            header(&response, header::CACHE_CONTROL),
            "public, max-age=31536000, immutable"
        );
        assert_security_headers(&response);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn unknown_and_traversal_paths_are_404() {
    let dir = temp_dist("missing", true);
    for path in [
        "/nope",
        "/accounts",
        "/assets/missing.js",
        "/../Cargo.toml",
        "/assets/../index.html",
        "/%2e%2e/Cargo.toml",
    ] {
        let web = WebAssets::from_dir(&dir).expect("dist");
        let (response, _) = request(web, Method::GET, path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn only_get_and_head_are_allowed() {
    let (response, _) = request(WebAssets::placeholder(), Method::POST, "/").await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let (response, body) = request(WebAssets::placeholder(), Method::HEAD, "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.is_empty());
}

#[tokio::test]
async fn unknown_api_paths_stay_json_404() {
    let dir = temp_dist("api", true);
    for (method, path) in [
        (Method::GET, "/api"),
        (Method::GET, "/api/"),
        (Method::GET, "/api/nope"),
        (Method::GET, "/api/v2/meta"),
        (Method::GET, "/api/v1/nope"),
        (Method::POST, "/api/v1/nope/deeper"),
        (Method::GET, "/api/index.html"),
    ] {
        let web = WebAssets::from_dir(&dir).expect("dist");
        let (response, body) = request(web, method, path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            header(&response, header::CONTENT_TYPE),
            "application/json",
            "{path}"
        );
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["code"], "not_found", "{path}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn placeholder_page_without_a_web_build() {
    let (response, body) = request(WebAssets::placeholder(), Method::GET, "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(&response, header::CONTENT_TYPE),
        "text/html; charset=utf-8"
    );
    assert_security_headers(&response);
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("SOLDE_WEB_DIR"));
    assert!(!body.contains("<script"), "no inline script under the CSP");
}

#[test]
fn a_web_dir_without_index_is_refused() {
    let dir = temp_dist("noindex", false);
    let err = WebAssets::from_dir(&dir).err().expect("refused");
    assert!(format!("{err:#}").contains("index.html"));
    let err = WebAssets::from_dir(&dir.join("missing"))
        .err()
        .expect("refused");
    assert!(format!("{err:#}").contains("SOLDE_WEB_DIR"));
    let _ = std::fs::remove_dir_all(&dir);
}

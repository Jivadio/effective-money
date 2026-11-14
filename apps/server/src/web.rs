// SPDX-License-Identifier: AGPL-3.0-or-later
//! The web app (`apps/web/dist`), served from memory.
//!
//! The files are read once at startup from `SOLDE_WEB_DIR` (the Docker image
//! sets it to the copy baked into the image). Requests are then answered by
//! a map lookup: no file system access per request, so no path traversal.
//! Without `SOLDE_WEB_DIR`, `/` answers a small placeholder page, which keeps
//! `cargo build` and `cargo run` working without a Node toolchain.
//!
//! - `assets/*` (content-hashed by Vite): cached for a year, `immutable`.
//! - everything else, `index.html` included: `no-cache`.
//! - every answer: a strict CSP (no inline script or style), `nosniff`,
//!   `Referrer-Policy: no-referrer`, `frame-ancestors 'none'`.
//!
//! Routing in the app uses the URL hash, so the server never needs to map
//! app paths to `index.html`; unknown paths are plain 404s.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

/// Content Security Policy of every static answer. Vite emits external
/// module scripts and stylesheets only, and React sets styles through the
/// CSSOM, so no `'unsafe-inline'` is needed.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; \
     style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; \
     manifest-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const NO_CACHE: &str = "no-cache";

const PLACEHOLDER: &str = "<!doctype html>
<html lang=\"en\">
  <head>
    <meta charset=\"utf-8\" />
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
    <title>Solde</title>
  </head>
  <body>
    <h1>Solde</h1>
    <p>The server is running, but this build does not include the web app.</p>
    <p>Build it with <code>pnpm --dir apps/web build</code> and start the server with
    <code>SOLDE_WEB_DIR=apps/web/dist</code>, or use the Docker image, which includes it.</p>
  </body>
</html>
";

struct Asset {
    body: Bytes,
    content_type: &'static str,
    cache_control: &'static str,
}

/// The web app files, keyed by path relative to the app root
/// (`index.html`, `assets/index-abc123.js`).
pub struct WebAssets {
    files: HashMap<String, Asset>,
}

impl WebAssets {
    /// A single placeholder `index.html`, for builds without the web app.
    pub fn placeholder() -> Self {
        let mut files = HashMap::new();
        files.insert(
            "index.html".to_owned(),
            asset("index.html", Bytes::from_static(PLACEHOLDER.as_bytes())),
        );
        WebAssets { files }
    }

    /// Reads every regular file under `dir` (a Vite `dist` directory).
    /// Fails if `dir` has no `index.html`.
    pub fn from_dir(dir: &Path) -> anyhow::Result<Self> {
        let mut files = HashMap::new();
        read_dir(dir, "", &mut files).with_context(|| {
            format!(
                "cannot read the web app in SOLDE_WEB_DIR ({})",
                dir.display()
            )
        })?;
        anyhow::ensure!(
            files.contains_key("index.html"),
            "SOLDE_WEB_DIR ({}) has no index.html: build the web app with \
             `pnpm --dir apps/web build` and point SOLDE_WEB_DIR at apps/web/dist",
            dir.display()
        );
        Ok(WebAssets { files })
    }

    /// The answer to a non-API request.
    pub fn respond(&self, method: &Method, uri: &Uri) -> Response {
        if method != Method::GET && method != Method::HEAD {
            let mut response =
                with_security_headers(StatusCode::METHOD_NOT_ALLOWED.into_response());
            response
                .headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
            return response;
        }
        let path = uri.path().trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };
        let Some(asset) = self.files.get(path) else {
            return with_security_headers(StatusCode::NOT_FOUND.into_response());
        };
        let mut response = Response::new(Body::from(asset.body.clone()));
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(asset.content_type),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(asset.cache_control),
        );
        with_security_headers(response)
    }

    /// Router fallback serving these files.
    pub fn into_fallback(
        self,
    ) -> impl Fn(Method, Uri) -> std::future::Ready<Response> + Clone + Send + Sync + 'static {
        let assets = Arc::new(self);
        move |method, uri| std::future::ready(assets.respond(&method, &uri))
    }
}

fn with_security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}

fn read_dir(dir: &Path, prefix: &str, files: &mut HashMap<String, Asset>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let key = format!("{prefix}{name}");
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            read_dir(&entry.path(), &format!("{key}/"), files)?;
        } else if file_type.is_file() {
            let body = Bytes::from(std::fs::read(entry.path())?);
            files.insert(key.clone(), asset(&key, body));
        }
    }
    Ok(())
}

fn asset(path: &str, body: Bytes) -> Asset {
    Asset {
        body,
        content_type: content_type(path),
        cache_control: if path.starts_with("assets/") {
            IMMUTABLE
        } else {
            NO_CACHE
        },
    }
}

fn content_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    match extension.to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "txt" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

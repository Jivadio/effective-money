// SPDX-License-Identifier: AGPL-3.0-or-later
//! Solde server: wires the API, MCP and sync layers into one binary.
//!
//! For now it only exposes `GET /healthz`.

use std::backtrace::Backtrace;
use std::path::PathBuf;

use axum::{Json, Router, routing::get};
use serde::Serialize;

// Linked now so the dependency rules apply to the real binary; used once the
// API, MCP and sync layers have content.
use solde_api as _;
use solde_mcp as _;
use solde_sync as _;

/// Server version, reported by `/healthz`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Body of `GET /healthz`.
#[derive(Debug, Serialize)]
pub struct Health {
    /// Always `"ok"` when the process can serve requests.
    pub status: &'static str,
    /// Server version.
    pub version: &'static str,
}

async fn healthz() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: VERSION,
    })
}

/// Builds the HTTP router.
pub fn app() -> Router {
    Router::new().route("/healthz", get(healthz))
}

/// Initializes `tracing` (filter from `RUST_LOG`, default `info`; JSON output
/// when `SOLDE_LOG_FORMAT=json`).
pub fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let json = std::env::var("SOLDE_LOG_FORMAT").is_ok_and(|v| v.eq_ignore_ascii_case("json"));
    let builder = fmt().with_env_filter(filter);
    if json {
        builder.json().init();
    } else {
        builder.init();
    }
}

/// Installs a panic hook that logs the panic location and a backtrace.
///
/// The panic **payload is deliberately not logged**: it may contain secrets or
/// financial data formatted by the panicking code.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_owned());
        let backtrace = Backtrace::force_capture();
        tracing::error!(%location, %backtrace, "panic (payload redacted)");
    }));
}

/// Validates the master-key file location if one is configured.
///
/// The key itself is not read yet (the vault arrives with the storage layer). If
/// `SOLDE_MASTER_KEY_FILE` is set, the file must be a readable regular file,
/// otherwise startup fails.
pub fn check_master_key_file() -> anyhow::Result<Option<PathBuf>> {
    let Some(path) = std::env::var_os("SOLDE_MASTER_KEY_FILE").map(PathBuf::from) else {
        return Ok(None);
    };
    let file = std::fs::File::open(&path).map_err(|err| {
        anyhow::anyhow!(
            "SOLDE_MASTER_KEY_FILE points to {}, which cannot be opened: {err}",
            path.display()
        )
    })?;
    if !file.metadata()?.is_file() {
        anyhow::bail!(
            "SOLDE_MASTER_KEY_FILE points to {}, which is not a regular file",
            path.display()
        );
    }
    Ok(Some(path))
}

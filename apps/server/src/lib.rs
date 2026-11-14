// SPDX-License-Identifier: AGPL-3.0-or-later
//! Solde server: wires the API, MCP and sync layers into one binary.
//!
//! It serves `GET /healthz`, the REST API under `/api/v1` and the web app
//! (see [`web`]).

use std::backtrace::Backtrace;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use axum::{Json, Router, routing::get};
use serde::Serialize;
use solde_api::{ApiConfig, SetupCode, TrustedProxies};
use solde_services::Store;
use solde_sync::MasterKey;

// Linked now so the dependency rules apply to the real binary; used once
// the MCP layer has content.
use solde_mcp as _;

pub mod web;

pub use web::WebAssets;

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

/// Builds the HTTP router: `/healthz`, the API, and the web app for every
/// other path. Unknown `/api` paths answer the API's JSON 404, never the
/// web app. Fails if the API cannot start (its password hasher).
pub fn app(
    store: Arc<dyn Store>,
    api: ApiConfig,
    web: WebAssets,
) -> Result<Router, solde_api::InitError> {
    Ok(Router::new()
        .route("/healthz", get(healthz))
        .merge(solde_api::router(store, api)?)
        .fallback(web.into_fallback()))
}

/// Settings read from the environment.
pub struct Config {
    /// `SOLDE_LISTEN_ADDR`, default `0.0.0.0:8080`.
    pub listen_addr: SocketAddr,
    /// `SOLDE_DATABASE_URL`: `postgres://...` or `sqlite://...`. Never
    /// logged (it may hold a password).
    pub database_url: String,
    /// `SOLDE_MASTER_KEY_FILE`.
    pub master_key_file: Option<PathBuf>,
    /// `SOLDE_WEB_DIR`: the built web app (`apps/web/dist`). Unset, the
    /// server answers `/` with a placeholder page.
    pub web_dir: Option<PathBuf>,
    /// `SOLDE_INSECURE_COOKIES` and `SOLDE_TRUSTED_PROXIES`. The setup
    /// code is added at startup, see [`setup_code`].
    pub api: ApiConfig,
}

/// How to provide a master key, shown when none is available.
pub const NO_KEY_HELP: &str = "no master key: set SOLDE_MASTER_KEY_FILE to a file holding 32 \
     random bytes in base64, e.g. `head -c 32 /dev/urandom | base64 > master.key` \
     (with Docker Compose the `keygen` service creates it). Keep a copy somewhere \
     safe: without it, saved bank credentials cannot be decrypted";

impl Config {
    /// Reads the settings from the process environment.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// Reads the settings through `var` (the environment, or a test map).
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let listen_addr = var("SOLDE_LISTEN_ADDR")
            .unwrap_or_else(|| "0.0.0.0:8080".to_owned())
            .parse()
            .context("SOLDE_LISTEN_ADDR must be a socket address, e.g. 0.0.0.0:8080")?;
        let database_url = var("SOLDE_DATABASE_URL")
            .filter(|url| !url.trim().is_empty())
            .context(
                "SOLDE_DATABASE_URL is not set: use postgres://user:password@host:5432/solde \
                 or sqlite:///path/to/solde.db",
            )?;
        let master_key_file = var("SOLDE_MASTER_KEY_FILE")
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from);
        let web_dir = var("SOLDE_WEB_DIR")
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from);
        let insecure_cookies = match var("SOLDE_INSECURE_COOKIES")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "0" | "false" | "no" => false,
            "1" | "true" | "yes" => true,
            _ => anyhow::bail!("SOLDE_INSECURE_COOKIES must be 1 or 0"),
        };
        let trusted_proxies = TrustedProxies::parse(
            &var("SOLDE_TRUSTED_PROXIES").unwrap_or_default(),
        )
        .context("SOLDE_TRUSTED_PROXIES must be a comma-separated list of IPs or CIDR ranges")?;
        Ok(Config {
            listen_addr,
            database_url,
            master_key_file,
            web_dir,
            api: ApiConfig {
                insecure_cookies,
                trusted_proxies,
                setup_code: None,
            },
        })
    }
}

/// The one-time setup code for a store that is not set up yet, logged once
/// at `warn` with instructions; `None` once the first admin exists.
///
/// This is the one secret the server ever logs, on purpose: it is the only
/// channel a self-hoster is sure to have, and the code is useless after
/// setup. It proves that whoever creates the household can read the
/// server's log, so a stranger who reaches a fresh instance first cannot
/// claim it.
pub async fn setup_code(store: &dyn Store) -> anyhow::Result<Option<SetupCode>> {
    if solde_services::is_bootstrapped(store)
        .await
        .context("cannot read whether the instance is set up")?
    {
        return Ok(None);
    }
    let code = SetupCode::generate().context("cannot generate the setup code")?;
    tracing::warn!(
        "first start: open the web app to create your household, with this one-time \
         setup code: {} (it stops working once setup is done; restarting the server \
         prints a new one)",
        code.expose()
    );
    Ok(Some(code))
}

/// Loads the web app from `SOLDE_WEB_DIR`, or the placeholder page when it
/// is not set.
pub fn load_web_assets(dir: Option<&PathBuf>) -> anyhow::Result<WebAssets> {
    match dir {
        Some(dir) => WebAssets::from_dir(dir),
        None => Ok(WebAssets::placeholder()),
    }
}

/// Loads the master key from `SOLDE_MASTER_KEY_FILE`, or explains how to
/// provide one. The error never contains key material.
pub fn load_master_key(path: Option<&PathBuf>) -> anyhow::Result<MasterKey> {
    let path = path.context(NO_KEY_HELP)?;
    MasterKey::from_file(path).with_context(|| {
        format!(
            "cannot load the master key from SOLDE_MASTER_KEY_FILE ({})",
            path.display()
        )
    })
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

// SPDX-License-Identifier: AGPL-3.0-or-later
//! Solde server binary.
//!
//! - `solde-server`: serve the API and the web app (configuration from the
//!   environment).
//! - `solde-server admin export-key`: print the master key to stdout, for
//!   a backup. The key is never printed or logged otherwise.

use std::io::Write as _;
use std::net::SocketAddr;
use std::process::ExitCode;

use anyhow::Context as _;
use solde_server::Config;

const USAGE: &str = "\
usage: solde-server [admin export-key]

  (no command)        serve the API and web app; configured by SOLDE_* environment variables
  admin export-key    print the master key (SOLDE_MASTER_KEY_FILE) to stdout";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        [] => serve(),
        ["admin", "export-key"] => export_key(),
        ["help" | "--help" | "-h"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// Prints the key in its file form. Only when explicitly asked for.
fn export_key() -> anyhow::Result<()> {
    let path = std::env::var_os("SOLDE_MASTER_KEY_FILE").map(std::path::PathBuf::from);
    let key = solde_server::load_master_key(path.as_ref())?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{}", key.export_base64().as_str()).context("cannot write to stdout")?;
    stdout.flush().context("cannot write to stdout")?;
    Ok(())
}

fn serve() -> anyhow::Result<()> {
    solde_server::init_tracing();
    solde_server::install_panic_hook();

    let config = Config::from_env()?;
    // Refuse to start without a key, before touching the database. Held
    // for the whole run (the sync engine will own it from M3).
    let master_key = solde_server::load_master_key(config.master_key_file.as_ref())?;
    tracing::info!("master key loaded");
    let web = solde_server::load_web_assets(config.web_dir.as_ref())?;
    if config.web_dir.is_none() {
        tracing::warn!(
            "SOLDE_WEB_DIR is not set: serving a placeholder page instead of the web app"
        );
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?
        .block_on(run(config, web))?;
    drop(master_key);
    Ok(())
}

async fn run(mut config: Config, web: solde_server::WebAssets) -> anyhow::Result<()> {
    if config.api.insecure_cookies {
        tracing::warn!(
            "SOLDE_INSECURE_COOKIES is set: the session cookie is sent over plain HTTP. \
             Use this only on a trusted local network, never on the internet"
        );
    }
    if !config.api.trusted_proxies.is_empty() {
        tracing::info!("X-Forwarded-For is trusted from SOLDE_TRUSTED_PROXIES only");
    }

    // Opening the store applies pending migrations and refuses a database
    // migrated by a newer Solde.
    let store = solde_services::connect(&config.database_url)
        .await
        .context("cannot open the database named by SOLDE_DATABASE_URL")?;
    tracing::info!(
        backend = ?store.backend(),
        schema = store.schema_version().await.context("cannot read the schema version")?,
        "database ready"
    );

    config.api.setup_code = solde_server::setup_code(&*store).await?;
    let app = solde_server::app(store, config.api, web)?;

    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .with_context(|| format!("cannot bind {}", config.listen_addr))?;
    tracing::info!(addr = %config.listen_addr, version = solde_server::VERSION, "listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("server error")?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::warn!(%err, "cannot listen for ctrl-c");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(err) => {
                tracing::warn!(%err, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

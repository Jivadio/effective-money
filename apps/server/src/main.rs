// SPDX-License-Identifier: AGPL-3.0-or-later
//! effective-money server binary.

use std::net::SocketAddr;

use anyhow::Context as _;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    em_server::init_tracing();
    em_server::install_panic_hook();

    if let Some(path) = em_server::check_master_key_file()? {
        tracing::info!(path = %path.display(), "master key file present (not used yet)");
    }

    let addr: SocketAddr = std::env::var("EM_LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse()
        .context("EM_LISTEN_ADDR must be a socket address, e.g. 0.0.0.0:8080")?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot bind {addr}"))?;
    tracing::info!(%addr, version = em_server::VERSION, "listening");

    axum::serve(listener, em_server::app())
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

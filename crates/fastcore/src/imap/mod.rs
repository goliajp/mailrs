//! Fastcore-native IMAP server. See [`session`] for the state
//! machine + command handlers, [`backend`] for the kevy + maildir
//! backend, and [`spawn`] / [`spawn_tls`] for the listener wiring.

pub mod backend;
pub mod fetch;
mod mailbox;
mod query;
pub mod search_eval;
mod session;

use std::sync::Arc;

use mailrs_tls_reload::TlsState;
use tokio::net::TcpListener;

use crate::FastcoreState;

/// Bind plaintext IMAP on `MAILRS_IMAP_BIND` (default `0.0.0.0:143`).
/// Set the env to `off` to disable.
pub async fn spawn(state: Arc<FastcoreState>) {
    let bind = std::env::var("MAILRS_IMAP_BIND").unwrap_or_else(|_| "0.0.0.0:143".to_string());
    if bind.eq_ignore_ascii_case("off") || bind.is_empty() {
        tracing::debug!("MAILRS_IMAP_BIND=off — skipping IMAP listener");
        return;
    }
    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, %bind, "imap: bind failed; disabling IMAP");
            return;
        }
    };
    tracing::info!(%bind, "imap: listening");
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "imap: accept error");
                continue;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            tracing::debug!(%peer, "imap: connection open");
            session::run(state, sock).await;
            tracing::debug!(%peer, "imap: connection closed");
        });
    }
}

/// Bind implicit-TLS IMAPS on `MAILRS_IMAPS_BIND` (default
/// `0.0.0.0:993`) and wrap every accepted socket in whichever
/// certificate `tls` holds at that moment, so a renewal reaches new
/// connections without a restart.
pub async fn spawn_tls(state: Arc<FastcoreState>, tls: TlsState) {
    let bind = std::env::var("MAILRS_IMAPS_BIND").unwrap_or_else(|_| "0.0.0.0:993".to_string());
    if bind.eq_ignore_ascii_case("off") || bind.is_empty() {
        tracing::debug!("MAILRS_IMAPS_BIND=off — skipping IMAPS listener");
        return;
    }
    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, %bind, "imaps: bind failed");
            return;
        }
    };
    tracing::info!(%bind, "imaps: listening (implicit TLS)");
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "imaps: accept error");
                continue;
            }
        };
        let state = state.clone();
        let acceptor = tls.acceptor();
        tokio::spawn(async move {
            tracing::debug!(%peer, "imaps: connection open");
            match acceptor.accept(sock).await {
                Ok(tls_sock) => session::run(state, tls_sock).await,
                Err(e) => tracing::warn!(%peer, error = %e, "imaps: handshake failed"),
            }
            tracing::debug!(%peer, "imaps: connection closed");
        });
    }
}

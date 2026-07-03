//! Fastcore-native IMAP server. See [`session`] for the state
//! machine + command handlers, [`backend`] for the kevy + maildir
//! backend, and [`spawn`] for the listener wiring.

pub mod backend;
pub mod session;

use std::sync::Arc;

use tokio::net::TcpListener;

use crate::FastcoreState;

/// Bind IMAP on the address given by `MAILRS_IMAP_BIND`
/// (default `0.0.0.0:143`) and accept connections in a loop. Env-
/// controlled so operators can disable per-container by setting
/// `MAILRS_IMAP_BIND=off`.
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

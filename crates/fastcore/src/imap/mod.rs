//! Fastcore-native IMAP server. See [`session`] for the state
//! machine + command handlers, [`backend`] for the kevy + maildir
//! backend, and [`spawn`] / [`spawn_tls`] for the listener wiring.

pub mod backend;
pub mod session;

use std::path::Path;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

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
/// `0.0.0.0:993`). Loads the TLS cert / key from `MAILRS_TLS_CERT`
/// / `MAILRS_TLS_KEY` at startup and wraps every accepted socket in
/// a rustls acceptor before entering the session state machine.
/// Silently skipped when either the bind or the cert paths are unset.
pub async fn spawn_tls(state: Arc<FastcoreState>) {
    let bind = std::env::var("MAILRS_IMAPS_BIND").unwrap_or_else(|_| "0.0.0.0:993".to_string());
    if bind.eq_ignore_ascii_case("off") || bind.is_empty() {
        tracing::debug!("MAILRS_IMAPS_BIND=off — skipping IMAPS listener");
        return;
    }
    let (Ok(cert_path), Ok(key_path)) = (
        std::env::var("MAILRS_TLS_CERT"),
        std::env::var("MAILRS_TLS_KEY"),
    ) else {
        tracing::debug!("MAILRS_TLS_CERT / MAILRS_TLS_KEY unset — skipping IMAPS listener");
        return;
    };
    let acceptor = match load_tls_acceptor(Path::new(&cert_path), Path::new(&key_path)) {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, %cert_path, %key_path, "imaps: TLS config load failed");
            return;
        }
    };
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
        let acceptor = acceptor.clone();
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

/// Load rustls from PEM cert + key. Returns a `TlsAcceptor` cloneable
/// per-connection. Shared with POP3S — same cert files.
pub(crate) fn load_tls_acceptor(cert_path: &Path, key_path: &Path) -> std::io::Result<TlsAcceptor> {
    let cfg = mailrs_tls_reload::load_tls_config(cert_path, key_path)?;
    let cfg_owned = std::sync::Arc::try_unwrap(cfg).unwrap_or_else(|arc| (*arc).clone());
    Ok(TlsAcceptor::from(std::sync::Arc::new(cfg_owned)))
}

//! Binding the five listeners and accepting on them.
//!
//! Each address comes from its own `MAILRS_*_BIND` variable; `off` or an
//! empty value leaves that listener out. The implicit-TLS ports exist only
//! with a certificate. Every accepted socket is wrapped in whichever
//! certificate the [`TlsState`] holds at that moment, so a renewal reaches
//! new connections without a restart.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use mailrs_tls_reload::TlsState;
use tokio::net::{TcpListener, TcpStream};

use crate::backend::Directory;

/// Start IMAP (143, STARTTLS), POP3 (110, STLS) and ManageSieve (4190,
/// STARTTLS), and with a certificate IMAPS (993) and POP3S (995).
///
/// Without a certificate the plaintext ports accept logins in clear and
/// offer no upgrade.
pub fn spawn_all<D: Directory>(dir: Arc<D>, tls: Option<TlsState>) {
    tokio::spawn(crate::imap::spawn(dir.clone(), tls.clone()));
    tokio::spawn(crate::pop3::spawn(dir.clone(), tls.clone()));
    tokio::spawn(crate::managesieve::spawn(dir.clone(), tls.clone()));
    if let Some(tls) = tls {
        tokio::spawn(crate::imap::spawn_tls(dir.clone(), tls.clone()));
        tokio::spawn(crate::pop3::spawn_tls(dir, tls));
    }
}

/// The listener `var` names (default `default`), `None` when it is
/// turned off or cannot be bound; both are logged.
pub(crate) async fn bind(var: &str, default: &str, name: &str) -> Option<TcpListener> {
    let addr = std::env::var(var).unwrap_or_else(|_| default.to_string());
    if addr.eq_ignore_ascii_case("off") || addr.is_empty() {
        tracing::debug!("{var}=off — skipping {name}");
        return None;
    }
    match TcpListener::bind(&addr).await {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::error!(error = %e, bind = %addr, "{name}: bind failed; disabling {name}");
            None
        }
    }
}

/// Run `handle` on every accepted connection, each on its own task.
pub(crate) async fn accept<F, Fut>(listener: TcpListener, name: &'static str, handle: F)
where
    F: Fn(TcpStream, SocketAddr) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "{name}: accept error");
                continue;
            }
        };
        tracing::debug!(%peer, "{name}: connection open");
        tokio::spawn(handle(sock, peer));
    }
}

/// [`accept`], with the TLS handshake done before `handle` runs.
pub(crate) async fn accept_tls<F, Fut>(
    listener: TcpListener,
    name: &'static str,
    tls: TlsState,
    handle: F,
) where
    F: Fn(tokio_rustls::server::TlsStream<TcpStream>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let handle = Arc::new(handle);
    accept(listener, name, move |sock, peer| {
        let acceptor = tls.acceptor();
        let handle = handle.clone();
        async move {
            match acceptor.accept(sock).await {
                Ok(stream) => handle(stream).await,
                Err(e) => tracing::warn!(%peer, error = %e, "{name}: handshake failed"),
            }
        }
    })
    .await;
}

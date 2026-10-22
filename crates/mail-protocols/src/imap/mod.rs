//! IMAP4rev1 (RFC 3501) with IDLE, NAMESPACE, ENABLE, UNSELECT, MOVE,
//! SPECIAL-USE, SORT, QUOTA, CONDSTORE and QRESYNC: [`run`] and
//! [`run_upgradable`] drive one connection.

mod append;
mod auth;
mod commands;
mod fetch;
mod fetch_items;
mod flags;
mod folders;
mod idle;
mod mailbox;
mod query;
pub mod search_eval;
mod session;
mod status;
mod store;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use mailrs_tls_reload::TlsState;
use tokio::net::TcpListener;

use crate::backend::Directory;
use crate::listen;

pub use flags::{flags_to_imap, parse_imap_flags};
pub use session::{run, run_upgradable};

/// Plaintext IMAP on `MAILRS_IMAP_BIND` (default `0.0.0.0:143`).
///
/// With a certificate the port offers STARTTLS and refuses to log in
/// until it is used. Without one (dev) there is no way to encrypt, so
/// logins are accepted in clear and nothing advertises STARTTLS.
pub async fn spawn<D: Directory>(dir: Arc<D>, tls: Option<TlsState>) {
    let Some(listener) = listen::bind("MAILRS_IMAP_BIND", "0.0.0.0:143", "imap").await else {
        return;
    };
    if tls.is_none() {
        tracing::warn!("imap: no certificate — no STARTTLS, passwords are accepted in clear");
    }
    tracing::info!(starttls = tls.is_some(), "imap: listening");
    serve_plain(listener, dir, tls).await;
}

pub async fn serve_plain<D: Directory>(listener: TcpListener, dir: Arc<D>, tls: Option<TlsState>) {
    listen::accept(listener, "imap", move |sock, _peer| {
        let dir = dir.clone();
        let tls = tls.clone();
        async move {
            match tls {
                Some(tls) => run_upgradable(dir, sock, tls).await,
                None => run(dir, sock).await,
            }
        }
    })
    .await;
}

/// Implicit-TLS IMAPS on `MAILRS_IMAPS_BIND` (default `0.0.0.0:993`).
pub async fn spawn_tls<D: Directory>(dir: Arc<D>, tls: TlsState) {
    let Some(listener) = listen::bind("MAILRS_IMAPS_BIND", "0.0.0.0:993", "imaps").await else {
        return;
    };
    tracing::info!("imaps: listening (implicit TLS)");
    serve_tls(listener, dir, tls).await;
}

pub async fn serve_tls<D: Directory>(listener: TcpListener, dir: Arc<D>, tls: TlsState) {
    listen::accept_tls(listener, "imaps", tls, move |stream| {
        run(dir.clone(), stream)
    })
    .await;
}

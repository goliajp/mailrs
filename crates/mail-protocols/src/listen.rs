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

/// One listener: the variable naming its address and the address it
/// binds when the variable is unset.
#[derive(Debug, Clone, Copy)]
pub struct Listener {
    pub var: &'static str,
    pub default: &'static str,
    pub name: &'static str,
}

pub const IMAP: Listener = Listener {
    var: "MAILRS_IMAP_BIND",
    default: "0.0.0.0:143",
    name: "imap",
};
pub const IMAPS: Listener = Listener {
    var: "MAILRS_IMAPS_BIND",
    default: "0.0.0.0:993",
    name: "imaps",
};
pub const POP3: Listener = Listener {
    var: "MAILRS_POP3_BIND",
    default: "0.0.0.0:110",
    name: "pop3",
};
pub const POP3S: Listener = Listener {
    var: "MAILRS_POP3S_BIND",
    default: "0.0.0.0:995",
    name: "pop3s",
};
pub const MANAGESIEVE: Listener = Listener {
    var: "MAILRS_MANAGESIEVE_BIND",
    default: "0.0.0.0:4190",
    name: "managesieve",
};

impl Listener {
    /// The address it binds, `None` when it is turned off (`off` or an
    /// empty value).
    pub fn address(&self) -> Option<String> {
        let addr = std::env::var(self.var).unwrap_or_else(|_| self.default.to_string());
        match addr.is_empty() || addr.eq_ignore_ascii_case("off") {
            true => None,
            false => Some(addr),
        }
    }

    /// The port of [`address`](Self::address), which is what mail apps
    /// are told to connect to; `None` when it is off or names no port.
    pub fn port(&self) -> Option<u16> {
        self.address()?.rsplit_once(':')?.1.parse().ok()
    }
}

/// The listener, `None` when it is turned off or cannot be bound; both
/// are logged.
pub(crate) async fn bind(listener: Listener) -> Option<TcpListener> {
    let (var, name) = (listener.var, listener.name);
    let Some(addr) = listener.address() else {
        tracing::debug!("{var}=off — skipping {name}");
        return None;
    };
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
        tokio::spawn(crate::guard::serving(peer.ip(), handle(sock, peer)));
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

/// The IMAP endpoint a mail app is told to use: the implicit-TLS port
/// when that listener is on, else the plaintext one, upgraded with
/// STARTTLS. `None` when neither is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImapForClients {
    pub port: u16,
    pub implicit_tls: bool,
}

pub fn imap_for_clients() -> Option<ImapForClients> {
    if let Some(port) = IMAPS.port() {
        return Some(ImapForClients {
            port,
            implicit_tls: true,
        });
    }
    IMAP.port().map(|port| ImapForClients {
        port,
        implicit_tls: false,
    })
}

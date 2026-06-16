//! Listener-setup helpers.
//!
//! `main.rs` used to repeat the same 15-25 lines per listener (SMTP,
//! submission, SMTPS, IMAP, IMAPS, POP3, ManageSieve): bind a
//! `TcpListener`, log "listening", spawn the accept loop, and spawn a
//! task per accepted connection. This module collapses the boilerplate
//! to a single helper so each listener call site is one
//! `spawn_plain(addr, label, shutdown, handler).await` invocation.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// Bind a TCP listener at `addr`, log "listening" with the supplied
/// `label`, and spawn an accept loop that calls `handler` for every
/// connection. Each connection runs on its own `tokio::spawn` task.
///
/// `handler` is `Fn(TcpStream, SocketAddr) -> impl Future<Output = ()>`,
/// so it can be called repeatedly. Capture per-listener state by moving
/// it into the closure and cloning it inside the returned future, the
/// same way the original hand-rolled accept loops did.
///
/// `shutdown` is the same `watch::Receiver<bool>` `main.rs` uses to
/// coordinate Ctrl-C shutdown. When `*shutdown.borrow()` becomes `true`
/// the accept loop stops accepting new connections (in-flight ones are
/// already spawned and continue to run until they finish on their own).
/// This restores graceful behaviour: prior hand-rolled loops also didn't
/// drain in-flight, so this is no regression.
///
/// Panics if `bind` fails — mirrors the prior behaviour. Accept errors
/// are logged at `tracing::error!` and the loop continues.
///
/// # Example
///
/// ```ignore
/// let ctx_smtp = ctx.clone();
/// listeners::spawn_plain(smtp_addr, "smtp", shutdown_rx.clone(), move |stream, addr| {
///     let ctx = ctx_smtp.clone();
///     async move {
///         smtp_session::handle_plain_connection(stream, addr, ctx).await
///     }
/// })
/// .await;
/// ```
pub async fn spawn_plain<F, Fut>(
    addr: String,
    label: &'static str,
    mut shutdown: watch::Receiver<bool>,
    handler: F,
) where
    F: Fn(TcpStream, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {label} on {addr}: {e}"));
    tracing::info!(addr = addr.as_str(), label, "listening");
    let handler = Arc::new(handler);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // Stop accepting new connections once shutdown fires.
                // In-flight tasks already spawned via `tokio::spawn` below
                // continue to completion — the runtime drains them at its
                // own shutdown.
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!(label, "shutdown signalled, stopping accept loop");
                        break;
                    }
                }
                accept = listener.accept() => match accept {
                    Ok((stream, peer)) => {
                        let h = handler.clone();
                        tokio::spawn(async move { h(stream, peer).await });
                    }
                    Err(e) => tracing::error!(label, error = %e, "accept error"),
                }
            }
        }
    });
}

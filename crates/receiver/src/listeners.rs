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

use mailrs_auth_guard::allowlist::{Allowlist, DenialLog};
use mailrs_auth_guard::events::{Channel, Credential, LoginEvent, Outcome};

use crate::config::ReceiverConfig;
use crate::smtp_session::{ConnectionContext, handle_plain_connection, handle_tls_connection};

/// The receiver's bound listeners. They stop accepting when this is
/// stopped or dropped.
pub struct Receiver {
    pub smtp: SocketAddr,
    pub submission: SocketAddr,
    /// `None` when no certificate is configured.
    pub smtps: Option<SocketAddr>,
    shutdown: watch::Sender<bool>,
    ctx: Arc<ConnectionContext>,
}

impl Receiver {
    pub fn stop(self) {
        let _ = self.shutdown.send(true);
    }

    /// Close the submission connections of accounts whose password has
    /// been cleared; what runs each time the sign-out subscription is
    /// (re)made.
    pub async fn end_cleared(&self) {
        crate::run::end_cleared(&self.ctx).await;
    }
}

/// SMTP and submission, plain with STARTTLS when TLS is configured, and
/// SMTPS (implicit TLS) only when a certificate is.
pub(crate) async fn listen(ctx: Arc<ConnectionContext>, cfg: &ReceiverConfig) -> Receiver {
    // submission (587) and SMTPS (465) only: port 25 takes mail from the
    // whole internet by definition
    let allow = Allowlist::from_env(SUBMISSION_ALLOW)
        .unwrap_or_else(|e| panic!("{e}"))
        .map(Arc::new);
    if allow.is_some() {
        tracing::info!("{SUBMISSION_ALLOW} set: submission accepts only the listed sources");
    }
    let denials = Arc::new(DenialLog::default());
    let (shutdown, rx) = watch::channel(false);
    let plain = |ctx: Arc<ConnectionContext>| {
        move |stream, addr| handle_plain_connection(stream, addr, ctx.clone())
    };
    let smtp = spawn_plain(
        format!("0.0.0.0:{}", cfg.smtp_port),
        "smtp",
        rx.clone(),
        plain(ctx.clone()),
    )
    .await;
    let submission = spawn_plain(
        format!("0.0.0.0:{}", cfg.submission_port),
        "submission",
        rx.clone(),
        only_from(
            allow.clone(),
            denials.clone(),
            ctx.clone(),
            plain(ctx.clone()),
        ),
    )
    .await;
    let mut smtps = None;
    if ctx.tls_state.is_some() {
        let c = ctx.clone();
        let tls = move |stream, addr| handle_tls_connection(stream, addr, c.clone());
        let tls = only_from(allow, denials, ctx.clone(), tls);
        smtps = Some(spawn_plain(format!("0.0.0.0:{}", cfg.smtps_port), "smtps", rx, tls).await);
    }
    Receiver {
        smtp,
        submission,
        smtps,
        shutdown,
        ctx,
    }
}

/// The submission ports' source allowlist; unset accepts everyone.
pub const SUBMISSION_ALLOW: &str = "MAILRS_SUBMISSION_ALLOW";

/// `handler`, for sources `allow` names only. Anyone else is disconnected
/// before the greeting or the TLS handshake, and recorded once per source
/// per window.
fn only_from<F, Fut>(
    allow: Option<Arc<Allowlist>>,
    denials: Arc<DenialLog>,
    ctx: Arc<ConnectionContext>,
    handler: F,
) -> impl Fn(TcpStream, SocketAddr) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>>
+ Send
+ Sync
+ 'static
where
    F: Fn(TcpStream, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    move |stream, addr| {
        let refused = allow.as_ref().is_some_and(|a| !a.allows(addr.ip()));
        if !refused {
            return Box::pin(handler(stream, addr));
        }
        drop(stream);
        let first = denials.first(
            addr.ip().to_canonical(),
            mailrs_auth_guard::login::unix_now(),
        );
        let guard = ctx.auth_guard.clone();
        Box::pin(async move {
            if first {
                let event = LoginEvent::new(
                    Channel::Smtp,
                    Credential::Password,
                    addr.ip(),
                    "",
                    Outcome::DeniedByAllowlist,
                );
                guard.record(event).await;
            }
        })
    }
}

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
) -> SocketAddr
where
    F: Fn(TcpStream, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {label} on {addr}: {e}"));
    let bound = listener.local_addr().expect("bound address");
    tracing::info!(addr = %bound, label, "listening");
    let handler = Arc::new(handler);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // Stop accepting new connections once shutdown fires.
                // In-flight tasks already spawned via `tokio::spawn` below
                // continue to completion — the runtime drains them at its
                // own shutdown. An Err means the sender is gone (`run`
                // returned or panicked); it stays ready, so looping on would
                // spin a core.
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
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
    bound
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[tokio::test]
    async fn the_accept_loop_ends_when_the_shutdown_sender_is_dropped() {
        let addr = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap()
        };
        let (tx, rx) = tokio::sync::watch::channel(false);
        super::spawn_plain(addr.to_string(), "test", rx, |_, _| async {}).await;
        assert!(tokio::net::TcpStream::connect(addr).await.is_ok());
        drop(tx);
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(addr).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("still accepting after the sender was dropped");
    }
}

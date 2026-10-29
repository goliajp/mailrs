//! Stop between jobs, never inside one.
//!
//! In its container the sender is PID 1, and PID 1 ignores a SIGTERM it has
//! no handler for: every `docker stop` waited out the grace period and
//! ended in SIGKILL, possibly in the middle of an SMTP transaction. A job
//! cut there is recovered as in-flight and delivered again, so the
//! recipient can get it twice.

use tokio::sync::watch;

/// Flips to `true` on SIGTERM or Ctrl-C.
pub(crate) fn on_stop_signal() -> watch::Receiver<bool> {
    let (tx, rx) = watch::channel(false);
    tokio::spawn(async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        let _ = tx.send(true);
    });
    rx
}

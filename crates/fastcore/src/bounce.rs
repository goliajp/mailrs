//! Bounce DSN generation + hand-off queue (G9, RFC 3464).
//!
//! The sender process detects permanent failures but owns no maildir;
//! fastcore owns delivery. Hand-off is a network-kevy queue:
//!
//! ```text
//! LPUSH mailrs:bounce:pending <id>
//! HSET  mailrs:bounce:<id> recipient=<local sender> blob=<b64 DSN>
//! ```
//!
//! fastcore's bounce drain (`spawn_bounce_drain`) pops ids, writes the
//! DSN into the recipient's maildir and runs the normal ingest
//! write-through — threading, uid, realtime push all come along.
//!
//! Double-bounce protection: no DSN is generated when the failed
//! message's envelope sender is null (`<>`), a MAILER-DAEMON, or a
//! postmaster address; the DSN itself is composed so a bounce OF the
//! bounce would hit exactly that guard on the remote side too.

use std::sync::Arc;

use crate::FastcoreState;

pub use mailrs_intake::dsn::{compose_dsn, dsn_identity, dsn_identity_from, suppress_bounce};
pub use mailrs_intake::notice::BOUNCE_PENDING;

/// Drain the bounce hand-off queue: deliver each DSN into the local
/// recipient's maildir and run the standard ingest write-through.
/// Unknown recipients (not a kevy account, no maildir) are dropped
/// with a warn — a bounce must never itself bounce.
///
/// v2.2 §3 (2026-07-08): the outer 10 s polling sleep is gone —
/// `drain_one` blocks on `brpop(BOUNCE_PENDING, Some(10s))`, so a
/// newly-enqueued bounce is processed synchronously with the
/// producer's `LPUSH`; the timer only fires when the queue has been
/// empty for the whole 10 s window, at which point the task loops
/// straight back into another blocking `BRPOP`. No wall-clock
/// budget spent on polling an empty queue.
///
/// The loop runs on `spawn_blocking`, not on a runtime worker.
/// `drain_one` is synchronous and blocks for up to 10 s with no `.await`
/// anywhere in the loop, so a plain `tokio::spawn` pinned one worker
/// thread for the life of the process and never gave it back — a quarter
/// of the async capacity on a four-core host. It also made shutdown
/// impossible: a worker parked in a blocking syscall with no yield point
/// never observes the runtime's shutdown, so the process flushed kevy on
/// SIGTERM, logged that it had, and then sat there. Measured before this
/// change: 0.55 s to exit with no `MAILRS_KEVY_URL`, still alive after
/// 40 s with one set — which is the production configuration, so every
/// deploy was waiting out `docker stop`'s grace period and being killed.
///
/// A written rule already required this, and already listed this call
/// site as one of its compliant callers.
pub fn spawn_bounce_drain(state: Arc<FastcoreState>) -> Option<tokio::task::JoinHandle<()>> {
    let Some(url) = crate::live_sync::network_kevy_url() else {
        tracing::info!("no network kevy — bounce drain disabled");
        return None;
    };
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let core = Arc::new(crate::spool_drain::Fastcore::new(&state, &maildir_root));
    Some(mailrs_intake::notice::spawn_drain(core, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DSN delivered through the hand-off queue must be removable by
    /// deleting its conversation: the delete finds files through the
    /// thread's message list.
    #[test]
    fn a_delivered_dsn_is_found_by_deleting_its_thread() {
        let store = std::sync::Arc::new(
            kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
        );
        let state = Arc::new(crate::FastcoreState::new(
            mailrs_mailbox_kevy::KevyMailboxStore::new(store),
        ));
        let dsn = compose_dsn(
            "mx.test",
            "mock.example.test",
            "lihao@golia.test",
            "bounce@mock.example.test",
            "5.1.1",
            "550 simulated",
            b"Message-ID: <orig@golia.test>\r\nSubject: try\r\n\r\nhi",
        );
        crate::ingest_delivered_file(
            &state,
            "lihao@golia.test",
            "1792208880.Mdsn1.bounce",
            &dsn,
            "INBOX",
            None,
        );
        let (existed, refs) = state
            .mailbox
            .delete_thread("lihao@golia.test", "orig@golia.test")
            .unwrap();
        assert!(existed);
        assert_eq!(
            refs,
            ["1792208880.Mdsn1.bounce"],
            "the delete found no file to unlink"
        );
    }
}

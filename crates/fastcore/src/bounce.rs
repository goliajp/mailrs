//! Bounce DSN generation + hand-off queue (G9, RFC 3464).
//!
//! The sender process detects permanent failures but owns no maildir;
//! fastcore owns delivery. Hand-off is a network-kevy queue:
//!
//!   LPUSH mailrs:bounce:pending <id>
//!   HSET  mailrs:bounce:<id> recipient=<local sender> blob=<b64 DSN>
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

use base64::Engine as _;

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
pub fn spawn_bounce_drain(state: Arc<FastcoreState>) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let Some(url) = crate::live_sync::network_kevy_url() else {
            tracing::info!("no network kevy — bounce drain disabled");
            return;
        };
        let maildir_root =
            std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
        loop {
            drain_one(&state, &url, &maildir_root);
        }
    })
}

fn drain_one(state: &Arc<FastcoreState>, url: &str, maildir_root: &str) {
    let Ok(mut conn) = kevy_client::Connection::connect(url) else {
        return;
    };
    // First pop blocks up to 10 s (matches prior wake-up cadence); after
    // that first hit we drain remaining items with non-blocking RPOPs to
    // avoid re-arming the blocking timer per item on a bursty queue.
    let mut first = true;
    loop {
        let popped_bytes: Option<Vec<u8>> = if first {
            first = false;
            match conn.brpop(&[BOUNCE_PENDING], Some(std::time::Duration::from_secs(10))) {
                Ok(Some((_key, value))) => Some(value),
                Ok(None) | Err(_) => None,
            }
        } else {
            conn.rpop(BOUNCE_PENDING, 1)
                .unwrap_or_default()
                .into_iter()
                .next()
        };
        let Some(id_bytes) = popped_bytes else {
            return;
        };
        let Ok(id) = String::from_utf8(id_bytes) else {
            continue;
        };
        let key = crate::bounce_queue::entry_key(&id);
        let recipient = conn
            .hget(key.as_bytes(), b"recipient")
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        let blob = conn
            .hget(key.as_bytes(), b"blob")
            .ok()
            .flatten()
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok());
        let _ = conn.del(&[key.as_bytes()]);
        let Some(bytes) = blob else { continue };
        // local-account guard: never forward a DSN outward
        let known = state
            .mailbox
            .get_account_blob(&recipient)
            .ok()
            .flatten()
            .is_some();
        let Some((local, domain)) = recipient.split_once('@') else {
            continue;
        };
        let base = std::path::PathBuf::from(maildir_root)
            .join(domain)
            .join(local);
        if !known && !base.join("new").is_dir() {
            tracing::warn!(%recipient, "bounce for unknown local sender dropped");
            continue;
        }
        for leaf in ["cur", "new", "tmp"] {
            let _ = std::fs::create_dir_all(base.join(leaf));
        }
        let filename = format!(
            "{}.Mdsn{}.bounce",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            id
        );
        let target = base.join("new").join(&filename);
        if let Err(e) = std::fs::write(&target, &bytes) {
            tracing::warn!(error = %e, %recipient, "bounce maildir write failed");
            continue;
        }
        // Bounce DSNs are inbound-side notifications to the sender —
        // always route to INBOX, never Junk (v2.4.0 Phase 2).
        crate::ingest_delivered_file(state, &recipient, &filename, &bytes, "INBOX", None);
        crate::live_sync::audit_system("mail.bounce", &recipient, "DSN delivered to sender");
        tracing::info!(%recipient, %id, "bounce DSN delivered");
    }
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

//! Maintenance work off the async runtime that stops when its caller does.
//!
//! A started `spawn_blocking` task is not cancelled when the request
//! awaiting it is dropped, so a client that gave up (a timeout, ^C) left
//! the walk running to its end. [`run_blocking`] sets a flag when its
//! future is dropped; each walk asks [`Hangup::hung_up`] once per item.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) struct Hangup {
    flag: Arc<AtomicBool>,
    /// Test-only: reports a hang-up once this many checks have passed.
    checks_left: Option<AtomicU64>,
}

impl Hangup {
    /// Whether the request that started this work has gone away.
    pub(crate) fn hung_up(&self) -> bool {
        if let Some(left) = &self.checks_left
            && left
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                .is_err()
        {
            return true;
        }
        self.flag.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn never() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            checks_left: None,
        }
    }

    /// A caller that hangs up after `n` items have been let through.
    #[cfg(test)]
    pub(crate) fn after(n: u64) -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            checks_left: Some(AtomicU64::new(n)),
        }
    }
}

struct SetOnDrop(Arc<AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// `work` on the blocking pool, told through its [`Hangup`] when the
/// future awaiting it is dropped.
pub(crate) async fn run_blocking<T, F>(work: F) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce(&Hangup) -> T + Send + 'static,
{
    let flag = Arc::new(AtomicBool::new(false));
    let _guard = SetOnDrop(flag.clone());
    let hangup = Hangup {
        flag,
        checks_left: None,
    };
    tokio::task::spawn_blocking(move || work(&hangup)).await
}

/// `n` one-message conversations for `user`, each with a stored row and
/// no file behind it: enough for a walk to visit, nothing to read.
#[cfg(test)]
pub(crate) fn seed(state: &crate::FastcoreState, user: &str, n: u32) {
    state.mailbox.upsert_account(user, "{}").unwrap();
    for i in 0..n {
        let (tid, mid) = (format!("t{i}"), format!("m{i}@x"));
        let wire = serde_json::json!({
            "uid": i + 1, "sender": "a@x", "recipients": user, "subject": "s",
            "date": 1, "internal_date": 1, "size": 1, "flags": 0,
            "message_id": mid, "in_reply_to": "", "thread_id": tid, "modseq": 0,
        });
        let facts = mailrs_mailbox_kevy::UserMessageFacts {
            blob_ref: "missing-file",
            uid: i + 1,
            flags: 0,
            modseq: 0,
        };
        state
            .mailbox
            .upsert_user_message(
                user,
                &tid,
                &mid,
                1,
                &serde_json::to_vec(&wire).unwrap(),
                &facts,
            )
            .unwrap();
        state
            .mailbox
            .record_message_arrival(&crate::tests::arr(&tid, user, true))
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn dropping_the_request_stops_the_blocking_work() {
        let (tx, rx) = std::sync::mpsc::channel();
        let request = run_blocking(move |h| {
            let mut items = 0u64;
            while !h.hung_up() {
                items += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
            tx.send(items).unwrap();
        });
        // the client gives up while the work is still going
        assert!(
            tokio::time::timeout(Duration::from_millis(50), request)
                .await
                .is_err()
        );
        let items = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(items > 0);
    }

    #[tokio::test]
    async fn work_that_finishes_returns_its_value() {
        assert!(!run_blocking(|h| h.hung_up()).await.unwrap());
    }

    #[test]
    fn after_lets_n_items_through() {
        let h = Hangup::after(2);
        assert!(!h.hung_up());
        assert!(!h.hung_up());
        assert!(h.hung_up());
        assert!(!Hangup::never().hung_up());
    }
}

//! The declared counters: what backfills them, what reads them, and what
//! is left to check about them.
//!
//! There was a shadow here — `maintenance:count-shadow`, comparing the
//! engine's counts against the stored ones — and it is retired. Its job
//! was to prove the two agreed *before* the read moved, and it did:
//! 32,278 threads across thirteen accounts on production, every field,
//! zero differences (2026-08-15). Nothing writes the stored counters
//! since C5b-2, so the comparison now has one live side and one that is
//! permanently zero — a metric that cannot come out non-zero, which
//! `measure-before-you-cut-over` calls not a verification at all. It was
//! deleted rather than adjusted to keep reporting something.
//!
//! What survives is the part that still has two sides: the group column
//! either exists on a row or does not (`ungrouped_user_message_rows`,
//! `backfill`), and the two declared axis columns can still disagree
//! with the engine (`axes`).

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Total rows in the whole store that are missing the group column.
    ///
    /// Reported alongside the per-user walk because the debt is global while
    /// the comparison is not, and a backfill's progress is easier to read
    /// off one number than off thirteen.
    pub fn ungrouped_user_message_rows(&self) -> io::Result<(u64, u64)> {
        let mut total = 0u64;
        let mut ungrouped = 0u64;
        let pattern = format!("{}*", String::from_utf8_lossy(keys::USER_MESSAGE_PREFIX));
        for key in self.store().keys(Some(pattern.as_bytes()), None) {
            total += 1;
            let flat = self.store().hgetall(&key)?;
            if !flat
                .iter()
                .any(|(f, v)| f.as_slice() == keys::USER_MESSAGE_GROUP_FIELD && !v.is_empty())
            {
                ungrouped += 1;
            }
        }
        Ok((total, ungrouped))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::MessageArrival;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    pub(crate) fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
        s.ensure_thread_table();
        s.ensure_admin_indexes();
        s
    }

    pub(crate) fn arrive(s: &KevyMailboxStore, tid: &str, user: &str, unread: bool, own: bool) {
        s.record_message_arrival(&MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: if own { user } else { "other@z.com" },
            latest_date: 100,
            latest_preview: "",
            category: "inbox",
            unread,
            is_own: own,
        })
        .unwrap();
    }

    pub(crate) fn put(
        s: &KevyMailboxStore,
        user: &str,
        tid: &str,
        mid: &str,
        flags: u32,
        from: &str,
    ) {
        s.upsert_user_message(
            user,
            tid,
            mid,
            100,
            serde_json::json!({ "message_id": mid, "sender": from })
                .to_string()
                .as_bytes(),
            &crate::UserMessageFacts {
                blob_ref: "f.host",
                uid: 1,
                flags,
                modseq: 1,
            },
        )
        .unwrap();
    }

    /// The global debt counter sees the same row.
    #[test]
    fn the_debt_counter_finds_the_ungrouped_row() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");
        put(&s, u, "t1", "m2", 0, "other@z.com");
        assert_eq!(s.ungrouped_user_message_rows().unwrap(), (2, 0));

        s.store()
            .hdel(
                keys::user_message(u, "m1").as_bytes(),
                &[keys::USER_MESSAGE_GROUP_FIELD],
            )
            .unwrap();
        assert_eq!(s.ungrouped_user_message_rows().unwrap(), (2, 1));
    }
}

mod axes;
mod backfill;
mod read;

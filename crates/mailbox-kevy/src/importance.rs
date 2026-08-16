//! Thread-level importance verdict write path.
//!
//! Importance is a **derivation**, not a fact: it is recomputed from the
//! message plus the sender relationship every time a message arrives, and
//! can be rebuilt from scratch at any point. It therefore lives outside
//! the atomic arrival block — a reader briefly seeing the previous
//! verdict costs nothing, whereas threading two more fields through all
//! fourteen `MessageArrival` construction sites (most of which — mark
//! seen, move category, rethread, migrate — have no verdict to offer)
//! would couple unrelated call paths to this feature.
//!
//! The caller decides *when* to write: only for inbound messages. A
//! user's own reply must not restate the thread's importance, mirroring
//! the display-field rule in `record_message_arrival` (2026-07-18).

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Store the importance verdict on a thread row.
    ///
    /// `level` is the stable lowercase token (`critical` / `important` /
    /// `normal` / `low` / `noise`) and `score` the raw numeric verdict.
    /// An empty `level` is a no-op: callers that could not compute a
    /// verdict must leave whatever the row already holds rather than
    /// blanking it.
    /// The membership row carries the verdict too, and this writes both.
    /// Writing only the shared hash left 19,463 of 30,716 rows on
    /// production disagreeing about `importance_level` — a difference
    /// invisible while the list read the shared hash, and the whole list
    /// the moment it stops.
    pub fn set_thread_importance(
        &self,
        user: &str,
        thread_id: &str,
        level: &str,
        score: f64,
    ) -> io::Result<()> {
        if level.is_empty() {
            return Ok(());
        }
        let key = keys::thread(thread_id);
        let tu_key = keys::thread_user(user, thread_id);
        let score_s = score.to_string();
        self.store().atomic(|ctx| {
            let pairs: [(&[u8], &[u8]); 2] = [
                (b"importance_level", level.as_bytes()),
                (b"importance_score", score_s.as_bytes()),
            ];
            ctx.hset(key.as_bytes(), &pairs)?;
            // Only when the user already has a row: this must not
            // conjure membership out of a verdict.
            if ctx.hexists(tu_key.as_bytes(), b"tid")? {
                ctx.hset(tu_key.as_bytes(), &pairs)?;
            }
            Ok(())
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        // Reads are served from the declared table, so a test store
        // has to look like a booted one.
        s.ensure_thread_table();
        // The aggregate index that derives the counters, too — without
        // it every count reads zero, which looks exactly like a broken
        // count rather than a store that was never fully booted.
        s.ensure_admin_indexes();
        s
    }

    /// The verdict has to reach the membership row, which is what the
    /// list reads. Writing only the shared hash is what left 19,463
    /// production rows disagreeing.
    #[test]
    fn the_verdict_reaches_the_users_own_row() {
        let s = store();
        s.record_message_arrival(&crate::MessageArrival {
            thread_id: "t1",
            user: "u@x.com",
            subject: "Subj",
            senders_csv: "alice@x.com",
            latest_date: 100,
            latest_preview: "p",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
        s.set_thread_importance("u@x.com", "t1", "critical", 0.9)
            .unwrap();

        let mine = s
            .get_thread_for_user("u@x.com", "t1")
            .unwrap()
            .expect("membership row");
        assert_eq!(mine.importance_level, "critical");
        assert!((mine.importance_score - 0.9).abs() < f64::EPSILON);
    }

    /// A verdict is not membership: a user with no row does not gain one.
    #[test]
    fn a_verdict_does_not_conjure_a_row() {
        let s = store();
        s.set_thread_importance("nobody@x.com", "t9", "critical", 0.9)
            .unwrap();
        assert!(
            s.get_thread_for_user("nobody@x.com", "t9")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn writes_and_reads_back() {
        let s = store();
        s.set_thread_importance("u@x.com", "t1", "important", 0.65)
            .unwrap();
        let row = s.get_thread("t1").unwrap().expect("row exists");
        assert_eq!(row.importance_level, "important");
        assert!((row.importance_score - 0.65).abs() < 1e-9);
    }

    #[test]
    fn empty_level_leaves_existing_verdict_alone() {
        let s = store();
        s.set_thread_importance("u@x.com", "t1", "critical", 0.9)
            .unwrap();
        // A caller with nothing to say must not blank the row.
        s.set_thread_importance("u@x.com", "t1", "", 0.0).unwrap();
        let row = s.get_thread("t1").unwrap().expect("row exists");
        assert_eq!(row.importance_level, "critical");
        assert!((row.importance_score - 0.9).abs() < 1e-9);
    }

    #[test]
    fn later_verdict_overwrites() {
        let s = store();
        s.set_thread_importance("u@x.com", "t1", "low", 0.1)
            .unwrap();
        s.set_thread_importance("u@x.com", "t1", "important", 0.7)
            .unwrap();
        let row = s.get_thread("t1").unwrap().expect("row exists");
        assert_eq!(row.importance_level, "important");
    }
}

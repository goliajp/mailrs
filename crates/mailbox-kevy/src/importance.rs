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
//!
//! **One reader's verdict, on that reader's membership row only.** The
//! score reads the relationship between this user and the sender, so two
//! people on one conversation get different answers. It used to be
//! written to the shared thread hash as well, and every write that
//! derives a membership row from that hash (an arrival, a sent copy, a
//! rethread, the row refresh) copied it onto whichever member came next:
//! a second member's row showed the first member's score. The shared
//! hash no longer carries it and `thread_user_pairs` no longer emits it,
//! so the two functions here are the only writers.

use std::io;

use super::KevyMailboxStore;
use super::keys;

const FIELDS: [&[u8]; 2] = [b"importance_level", b"importance_score"];

impl KevyMailboxStore {
    /// Store one user's importance verdict on their membership row.
    ///
    /// `level` is the stable lowercase token (`critical` / `important` /
    /// `normal` / `low` / `noise`) and `score` the raw numeric verdict.
    /// An empty `level` is a no-op: callers that could not compute a
    /// verdict must leave whatever the row already holds rather than
    /// blanking it.
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
        let tu_key = keys::thread_user(user, thread_id);
        let score_s = score.to_string();
        self.store().atomic(|ctx| {
            // Only when the user already has a row: this must not
            // conjure membership out of a verdict.
            if ctx.hexists(tu_key.as_bytes(), b"tid")? {
                ctx.hset(
                    tu_key.as_bytes(),
                    &[
                        (FIELDS[0], level.as_bytes()),
                        (FIELDS[1], score_s.as_bytes()),
                    ],
                )?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Remove a user's verdict, so the row reads as unscored. Returns
    /// whether the row held one.
    ///
    /// For a row whose verdict was never this user's: there is nothing of
    /// theirs to put back, and "unscored" is the honest answer.
    pub fn clear_thread_importance(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        let tu_key = keys::thread_user(user, thread_id);
        let removed = self
            .store()
            .atomic(|ctx| ctx.hdel(tu_key.as_bytes(), &FIELDS))?;
        Ok(removed > 0)
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

    fn arrive(s: &KevyMailboxStore, user: &str, senders: &str, at: i64, is_own: bool) {
        s.record_message_arrival(&crate::MessageArrival {
            thread_id: "t1",
            user,
            subject: "Subj",
            senders_csv: senders,
            latest_date: at,
            latest_preview: "p",
            category: "inbox",
            unread: !is_own,
            is_own,
        })
        .unwrap();
    }

    fn verdict(s: &KevyMailboxStore, user: &str) -> (String, f64) {
        let row = s
            .get_thread_for_user(user, "t1")
            .unwrap()
            .expect("membership row");
        (row.importance_level, row.importance_score)
    }

    /// One reader's verdict is theirs. A second member whose row is created
    /// after the first member was scored, by mail that carries no verdict of
    /// its own (their own reply), must not show the first member's score.
    #[test]
    fn a_later_members_row_never_shows_an_earlier_members_score() {
        let s = store();
        arrive(&s, "alice@x.com", "carol@y.com", 100, false);
        s.set_thread_importance("alice@x.com", "t1", "critical", 0.9)
            .unwrap();
        arrive(&s, "bob@x.com", "bob@x.com", 200, true);

        assert_eq!(verdict(&s, "bob@x.com"), (String::new(), 0.0));
        assert_eq!(verdict(&s, "alice@x.com"), ("critical".into(), 0.9));
    }

    /// Nor the other way round: the second member's verdict does not reach
    /// the first member's row on the first member's next unscored arrival.
    #[test]
    fn an_unscored_arrival_keeps_the_readers_own_verdict() {
        let s = store();
        arrive(&s, "alice@x.com", "carol@y.com", 100, false);
        s.set_thread_importance("alice@x.com", "t1", "critical", 0.9)
            .unwrap();
        arrive(&s, "bob@x.com", "carol@y.com", 100, false);
        s.set_thread_importance("bob@x.com", "t1", "low", 0.1)
            .unwrap();
        arrive(&s, "alice@x.com", "alice@x.com", 300, true);

        assert_eq!(verdict(&s, "alice@x.com"), ("critical".into(), 0.9));
        assert_eq!(verdict(&s, "bob@x.com"), ("low".into(), 0.1));
    }

    /// The shared conversation hash carries no verdict: it has no user
    /// segment, so whatever it held would be somebody's.
    #[test]
    fn the_shared_hash_carries_no_verdict() {
        let s = store();
        arrive(&s, "alice@x.com", "carol@y.com", 100, false);
        s.set_thread_importance("alice@x.com", "t1", "critical", 0.9)
            .unwrap();
        let fields: Vec<Vec<u8>> = s
            .store()
            .hgetall(keys::thread("t1").as_bytes())
            .unwrap()
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        assert!(!fields.is_empty(), "the shared hash was never written");
        for f in [b"importance_level".as_slice(), b"importance_score"] {
            assert!(
                !fields.iter().any(|x| x == f),
                "{} on the shared hash",
                String::from_utf8_lossy(f)
            );
        }
    }

    /// Refreshing a member's row from the shared hash leaves their verdict.
    #[test]
    fn the_row_refresh_keeps_the_readers_verdict() {
        let s = store();
        arrive(&s, "alice@x.com", "carol@y.com", 100, false);
        s.set_thread_importance("alice@x.com", "t1", "important", 0.65)
            .unwrap();
        arrive(&s, "bob@x.com", "carol@y.com", 100, false);
        s.set_thread_importance("bob@x.com", "t1", "low", 0.1)
            .unwrap();
        s.backfill_thread_user("alice@x.com", 0, 100).unwrap();
        assert_eq!(verdict(&s, "alice@x.com"), ("important".into(), 0.65));
    }

    #[test]
    fn empty_level_leaves_existing_verdict_alone() {
        let s = store();
        arrive(&s, "u@x.com", "alice@x.com", 100, false);
        s.set_thread_importance("u@x.com", "t1", "critical", 0.9)
            .unwrap();
        // A caller with nothing to say must not blank the row.
        s.set_thread_importance("u@x.com", "t1", "", 0.0).unwrap();
        assert_eq!(verdict(&s, "u@x.com"), ("critical".into(), 0.9));
    }

    #[test]
    fn later_verdict_overwrites() {
        let s = store();
        arrive(&s, "u@x.com", "alice@x.com", 100, false);
        s.set_thread_importance("u@x.com", "t1", "low", 0.1)
            .unwrap();
        s.set_thread_importance("u@x.com", "t1", "important", 0.7)
            .unwrap();
        assert_eq!(verdict(&s, "u@x.com").0, "important");
    }

    /// Clearing removes the verdict, so the row reads as unscored, and
    /// says whether there was one.
    #[test]
    fn clearing_leaves_the_row_unscored() {
        let s = store();
        arrive(&s, "u@x.com", "alice@x.com", 100, false);
        s.set_thread_importance("u@x.com", "t1", "low", 0.1)
            .unwrap();
        assert!(s.clear_thread_importance("u@x.com", "t1").unwrap());
        assert_eq!(verdict(&s, "u@x.com"), (String::new(), 0.0));
        assert!(!s.clear_thread_importance("u@x.com", "t1").unwrap());
        assert!(
            s.get_thread_for_user("u@x.com", "t1").unwrap().is_some(),
            "clearing a verdict removed the row"
        );
    }
}

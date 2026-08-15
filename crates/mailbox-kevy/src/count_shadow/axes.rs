//! The two declared columns derived from a shared number.
//!
//! Stage C4. `sent_only` and `is_sender` are **columns of the declared
//! `threaduser` table** — the Inbox ORDERPATH excludes threads whose
//! `sent_only` is set, and the Sent axis keys on `is_sender`. So they do
//! not merely describe a thread; they decide which list it appears in.
//!
//! Both are computed in `thread_user_pairs` from the `ThreadRow` its
//! caller happened to hold, and the callers hold the **shared** hash:
//! `set_thread_date` reads `get_thread(tid)` and hands it straight to
//! `upsert_thread(user, &row)`. On a thread with one owner that is
//! harmless. On a thread with several, `row.count` and `row.sent_count`
//! are everybody's totals, so one owner's replies can set another owner's
//! `sent_only` — and a thread the second owner never wrote in drops out
//! of their Inbox, or appears in their Sent.
//!
//! That is a cross-user leak into a declared axis, not a rounding error,
//! and production had 160 multi-owner threads when it was last measured.
//!
//! **Measured before it is changed.** These columns decide list
//! membership, so getting the correction wrong empties somebody's Inbox.
//! The shadow separates the population that *can* exhibit the defect
//! (threads with more than one owner) from the one that cannot, because
//! a difference outside it means the correction itself is wrong —
//! `rules/measure-before-you-cut-over.md`.

use std::io;

use super::super::KevyMailboxStore;
use super::super::keys;

/// One user's comparison of the two axis columns.
#[derive(Debug, Default, Clone)]
pub struct AxisShadow {
    /// Threads walked.
    pub scanned: u64,
    /// Threads with more than one owner — the only ones the defect can
    /// reach.
    pub shared: u64,
    /// Stored `sent_only` disagrees with the engine's per-user answer.
    pub sent_only_differs: u64,
    /// …of those, on a thread with more than one owner.
    pub sent_only_differs_shared: u64,
    /// Stored `is_sender` disagrees.
    pub is_sender_differs: u64,
    /// …of those, on a multi-owner thread.
    pub is_sender_differs_shared: u64,
    /// Threads the index cannot answer for, which are compared against
    /// nothing and counted separately rather than as agreement.
    pub not_indexed: u64,
    /// A few of the disagreements, named.
    pub samples: Vec<serde_json::Value>,
}

impl KevyMailboxStore {
    /// Compare the two declared axis columns against the engine's
    /// per-user counts.
    pub fn shadow_axis_columns(
        &self,
        user: &str,
        offset: i64,
        limit: i64,
    ) -> io::Result<AxisShadow> {
        let mut out = AxisShadow::default();
        let accounts = self.list_account_addresses().unwrap_or_default();
        let tids = self.all_thread_ids_for_user(user)?;
        let start = offset.max(0) as usize;
        let end = (start + limit.max(0) as usize).min(tids.len());

        for tid in tids.get(start..end).unwrap_or(&[]) {
            out.scanned += 1;
            let Some((total, _unread, own)) = self.counts_from_index(user, tid) else {
                out.not_indexed += 1;
                continue;
            };
            let shared = self.thread_is_shared(&accounts, tid).unwrap_or(false);
            if shared {
                out.shared += 1;
            }

            // What the columns would say if they were derived from this
            // user's own messages. `sent_only` keeps its meaning exactly:
            // every message in the thread came from this user. `is_sender`
            // is "has written in it at all".
            let want_sent_only = total > 0 && own >= total;
            let want_is_sender = own > 0;

            let pairs = self
                .store()
                .hgetall(keys::thread_user(user, tid).as_bytes())?;
            let col = |name: &str| -> bool {
                pairs
                    .iter()
                    .find(|(f, _)| f.as_slice() == name.as_bytes())
                    .map(|(_, v)| v.as_slice() == b"1")
                    .unwrap_or(false)
            };
            let (has_sent_only, has_is_sender) = (col("sent_only"), col("is_sender"));

            if has_sent_only != want_sent_only {
                out.sent_only_differs += 1;
                out.sent_only_differs_shared += u64::from(shared);
            }
            if has_is_sender != want_is_sender {
                out.is_sender_differs += 1;
                out.is_sender_differs_shared += u64::from(shared);
            }
            if (has_sent_only != want_sent_only || has_is_sender != want_is_sender)
                && out.samples.len() < 20
            {
                out.samples.push(serde_json::json!({
                    "tid": tid,
                    "shared": shared,
                    "counts": { "total": total, "own": own },
                    "stored": { "sent_only": has_sent_only, "is_sender": has_is_sender },
                    "derived": { "sent_only": want_sent_only, "is_sender": want_is_sender },
                }));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::count_shadow::tests::*;

    /// A thread the user only ever sent in has `sent_only`; one they
    /// merely replied in does not — that distinction is what keeps a
    /// conversation in the Inbox, and reading it as "has ever sent"
    /// dropped 190 threads from one account's inbox on production.
    #[test]
    fn sent_only_means_every_message_and_is_sender_means_any() {
        let s = store();
        let u = "u@x.com";
        // Sent-only: both messages are the user's.
        arrive(&s, "sent", u, false, true);
        arrive(&s, "sent", u, false, true);
        put(&s, u, "sent", "s1", 1, "u@x.com");
        put(&s, u, "sent", "s2", 1, "u@x.com");
        assert_eq!(s.counts_from_index(u, "sent"), Some((2, 0, 2)));

        // Replied-in: one theirs, one the user's.
        arrive(&s, "conv", u, true, false);
        arrive(&s, "conv", u, false, true);
        put(&s, u, "conv", "c1", 0, "other@z.com");
        put(&s, u, "conv", "c2", 1, "u@x.com");
        assert_eq!(s.counts_from_index(u, "conv"), Some((2, 1, 1)));

        let r = s.shadow_axis_columns(u, 0, 100).unwrap();
        assert_eq!(r.scanned, 2);
        assert_eq!(r.not_indexed, 0);
    }

    /// A thread whose rows the index cannot see is counted apart rather
    /// than compared against nothing and called agreement.
    #[test]
    fn an_unindexed_thread_is_counted_apart() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");
        s.store()
            .hdel(
                keys::user_message(u, "m1").as_bytes(),
                &[keys::USER_MESSAGE_GROUP_FIELD],
            )
            .unwrap();

        let r = s.shadow_axis_columns(u, 0, 100).unwrap();
        assert_eq!((r.scanned, r.not_indexed), (1, 1));
        assert_eq!(r.sent_only_differs, 0, "it must not be reported either way");
    }

    /// **The defect this exists for.** A second owner's messages inflate
    /// the shared counters, and the column derived from them says this
    /// user sent everything in a thread they never wrote in.
    #[test]
    fn a_second_owners_messages_do_not_decide_this_users_column() {
        let s = store();
        let (a, b) = ("a@x.com", "b@x.com");
        // `thread_is_shared` asks which accounts hold a membership row, so
        // the accounts have to exist — without them every thread reads as
        // single-owner and the distinction this test is about disappears.
        for addr in [a, b] {
            s.upsert_account(addr, r#"{"active":true}"#).unwrap();
        }
        // Both own the thread. `a` received one; `b` sent one.
        arrive(&s, "t1", a, true, false);
        put(&s, a, "t1", "m1", 0, "other@z.com");
        arrive(&s, "t1", b, false, true);
        put(&s, b, "t1", "m2", 1, "b@x.com");

        // The engine answers each of them about their own copy.
        assert_eq!(s.counts_from_index(a, "t1"), Some((1, 1, 0)));
        assert_eq!(s.counts_from_index(b, "t1"), Some((1, 0, 1)));

        // Forge the leak: `a`'s column set from the shared totals, the way
        // `set_thread_date` does by handing `get_thread`'s row to
        // `upsert_thread`.
        s.store()
            .hset(
                keys::thread_user(a, "t1").as_bytes(),
                &[(b"sent_only".as_slice(), b"1".as_slice())],
            )
            .unwrap();

        let r = s.shadow_axis_columns(a, 0, 100).unwrap();
        assert_eq!(r.sent_only_differs, 1, "the leak has to be reported");
        assert_eq!(
            r.sent_only_differs_shared, 1,
            "and reported as living on a multi-owner thread"
        );
        assert_eq!(r.samples.len(), 1);
        assert_eq!(r.samples[0]["stored"]["sent_only"], true);
        assert_eq!(r.samples[0]["derived"]["sent_only"], false);
    }
}

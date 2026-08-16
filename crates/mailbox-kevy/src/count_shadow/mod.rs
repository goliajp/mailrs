//! What the engine counts, against what the rows say — read-only.
//!
//! Stage C2. The declared aggregate index is written and not yet read, so
//! this compares the two before anything depends on the first.
//!
//! **The first number will be large, and that is not the defect.** Every
//! row written before the group column existed is missing it, so the index
//! has nothing to group it under and the engine counts zero for the thread.
//! That is a migration debt. `maintenance:threadrow-shadow` reported 19,779
//! differences on 2026-08-02 for exactly this reason and converged to 74
//! once the backfill ran — and 74 was the defect's real size. Reading the
//! first figure as a fault would have shipped a bigger fault than the one
//! being repaired (`rules/measure-before-you-cut-over.md`).
//!
//! So the report separates the two populations rather than adding them:
//!
//!   * `threads_with_ungrouped_rows` / `ungrouped_rows` — never backfilled.
//!     A debt. C3 drives it to zero.
//!   * `differs_with_every_row_grouped` — a thread whose rows *all* carry a
//!     group and whose numbers still disagree. Only this can be read as a
//!     fault, and only this gates the read cutover.
//!
//! Per field, never merged: `count`, `unread_count` and `sent_count` drift
//! for different reasons, and one total cannot say which.

use std::io;

use super::KevyMailboxStore;
use super::keys;
use crate::messages::State;

/// One user's comparison.
#[derive(Debug, Default, Clone)]
pub struct CountShadow {
    /// Threads walked.
    pub scanned: u64,
    /// Threads where all three numbers match.
    pub agreed: u64,
    /// Per-field disagreement, over every thread scanned.
    pub count_differs: u64,
    /// As above, for the unread counter.
    pub unread_differs: u64,
    /// As above, for the sent counter.
    pub sent_differs: u64,
    /// Threads holding at least one row without a group column.
    pub threads_with_ungrouped_rows: u64,
    /// Rows without a group column, across every thread scanned.
    pub ungrouped_rows: u64,
    /// **The number that gates the cutover.** Threads whose rows all carry
    /// a group and whose numbers still disagree.
    pub differs_with_every_row_grouped: u64,
    /// A few of those, named, so the disagreement can be looked at rather
    /// than only counted.
    pub samples: Vec<serde_json::Value>,
}

impl KevyMailboxStore {
    /// Compare engine-counted against stored, for one user.
    pub fn shadow_declared_counts(
        &self,
        user: &str,
        offset: i64,
        limit: i64,
    ) -> io::Result<CountShadow> {
        let mut out = CountShadow::default();
        let tids = self.all_thread_ids_for_user(user)?;
        let start = offset.max(0) as usize;
        let end = (start + limit.max(0) as usize).min(tids.len());

        for tid in tids.get(start..end).unwrap_or(&[]) {
            // **Read the stored fields off the hash, not through
            // `get_thread_for_user`.** That reader overlays the index's
            // counts (C5b), so going through it would compare the index
            // against itself — a verification that cannot come out
            // non-zero, which `measure-before-you-cut-over` calls not a
            // verification at all. Two of these tests went red the moment
            // the overlay landed, which is the only reason this is a
            // comment rather than a defect.
            let stored_pairs = self
                .store()
                .hgetall(keys::thread_user(user, tid).as_bytes())?;
            if stored_pairs.is_empty() {
                continue;
            }
            let stored_field = |name: &[u8]| -> i64 {
                stored_pairs
                    .iter()
                    .find(|(f, _)| f.as_slice() == name)
                    .and_then(|(_, v)| String::from_utf8_lossy(v).parse().ok())
                    .unwrap_or(0)
            };
            out.scanned += 1;

            // How many of this thread's rows the index can see at all. A row
            // without the column is invisible to the group counts below, and
            // invisible in a way that reads as a healthy number: both sides
            // answer, and the engine's is confidently short.
            let mut ungrouped = 0u64;
            for mid in self.user_thread_message_ids(user, tid)? {
                let flat = self
                    .store()
                    .hgetall(keys::user_message(user, &mid).as_bytes())?;
                let has_group = flat
                    .iter()
                    .any(|(f, v)| f.as_slice() == keys::USER_MESSAGE_GROUP_FIELD && !v.is_empty());
                if !has_group {
                    ungrouped += 1;
                }
            }
            out.ungrouped_rows += ungrouped;
            if ungrouped > 0 {
                out.threads_with_ungrouped_rows += 1;
            }

            let g = |st: State| -> u64 {
                self.store()
                    .idx_group(
                        keys::IDX_USERMSG_COUNTS,
                        crate::messages::group_name(user, tid, st).as_bytes(),
                    )
                    .map(|s| s.count)
                    .unwrap_or(0)
            };
            let (unread, read, own) = (g(State::Unread), g(State::Read), g(State::Own));
            let counted = (unread + read + own, unread, own);
            let stored = (
                stored_field(b"count").max(0) as u64,
                stored_field(b"unread_count").max(0) as u64,
                stored_field(b"sent_count").max(0) as u64,
            );

            let differs = (
                counted.0 != stored.0,
                counted.1 != stored.1,
                counted.2 != stored.2,
            );
            out.count_differs += u64::from(differs.0);
            out.unread_differs += u64::from(differs.1);
            out.sent_differs += u64::from(differs.2);

            let any = differs.0 || differs.1 || differs.2;
            if !any {
                out.agreed += 1;
            } else if ungrouped == 0 {
                out.differs_with_every_row_grouped += 1;
                if out.samples.len() < 20 {
                    out.samples.push(serde_json::json!({
                        "tid": tid,
                        "counted": { "count": counted.0, "unread": counted.1, "sent": counted.2 },
                        "stored":  { "count": stored.0,  "unread": stored.1,  "sent": stored.2 },
                    }));
                }
            }
        }
        Ok(out)
    }

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

    /// A thread whose rows all carry a group, and whose numbers match,
    /// reports as agreement and contributes to neither population.
    #[test]
    fn a_thread_that_agrees_is_reported_as_agreeing() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");

        let r = s.shadow_declared_counts(u, 0, 100).unwrap();
        assert_eq!((r.scanned, r.agreed), (1, 1));
        assert_eq!(r.ungrouped_rows, 0);
        assert_eq!(r.differs_with_every_row_grouped, 0);
    }

    /// **The distinction the whole route exists for.** A row written before
    /// the group column existed makes the engine count short — and that is
    /// a debt, not a defect, so it must not land in the number that gates
    /// the cutover.
    #[test]
    fn an_ungrouped_row_is_a_debt_and_not_a_defect() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");
        // Strip the column the way a pre-migration row lacks it.
        s.store()
            .hdel(
                keys::user_message(u, "m1").as_bytes(),
                &[keys::USER_MESSAGE_GROUP_FIELD],
            )
            .unwrap();

        let r = s.shadow_declared_counts(u, 0, 100).unwrap();
        assert_eq!(r.ungrouped_rows, 1, "the stripped row has to be counted");
        assert_eq!(r.threads_with_ungrouped_rows, 1);
        assert!(r.count_differs > 0, "the engine does count short");
        assert_eq!(
            r.differs_with_every_row_grouped, 0,
            "a debt must not be reported as a defect"
        );
        assert!(r.samples.is_empty(), "and must not be sampled as one");
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

    /// A real disagreement — every row grouped, stored counter wrong —
    /// reaches the number that gates the cutover, and is sampled.
    #[test]
    fn a_stored_counter_that_lies_is_reported_as_a_defect() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");
        // Drift the stored side, which is exactly what A3 found 127 of.
        s.store()
            .hset(
                keys::thread_user(u, "t1").as_bytes(),
                &[(b"count".as_slice(), b"9".as_slice())],
            )
            .unwrap();

        let r = s.shadow_declared_counts(u, 0, 100).unwrap();
        assert_eq!(r.count_differs, 1);
        assert_eq!(r.differs_with_every_row_grouped, 1);
        assert_eq!(r.samples.len(), 1);
        assert_eq!(r.samples[0]["stored"]["count"], 9);
        assert_eq!(r.samples[0]["counted"]["count"], 1);
    }

    /// Per field, not merged: a thread can agree on the total and disagree
    /// on which of them are unread, and the report has to say so.
    #[test]
    fn the_fields_are_reported_separately() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");
        s.store()
            .hset(
                keys::thread_user(u, "t1").as_bytes(),
                &[(b"unread_count".as_slice(), b"7".as_slice())],
            )
            .unwrap();

        let r = s.shadow_declared_counts(u, 0, 100).unwrap();
        assert_eq!(
            (r.count_differs, r.unread_differs, r.sent_differs),
            (0, 1, 0),
            "only the unread field disagrees"
        );
    }
}

mod axes;
mod backfill;
mod read;

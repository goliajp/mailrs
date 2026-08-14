//! Compare the two copies of the thread counters before reading from
//! the new one (RFC 20260730 S3).
//!
//! S1 writes `count` / `unread_count` / `sent_count` per user on the
//! membership row while the read path still serves them from the shared
//! `mailrs:thread:<tid>`. S4 flips the read. This answers whether it is
//! safe to.
//!
//! The interesting part is that **divergence is not one number**:
//!
//! - On a thread only one local account holds, the two copies describe
//!   the same thing and any disagreement is a bug in S1's maintenance —
//!   a writer that updated one copy and not the other.
//! - On a thread two local accounts hold, they *must* disagree. The
//!   shared row sums both deliveries and attributes the sender's own
//!   copy to whoever reads it; the per-user row is each mailbox's own
//!   view. That disagreement is the defect being fixed, showing up as
//!   intended.
//!
//! Reported as one total those cancel out into a number nobody can act
//! on. The gate for S4 is `diverged_single` at zero, with
//! `diverged_shared` free to be however large the double-counting was.

use std::io;

use super::KevyMailboxStore;
use super::keys;

/// Per-thread counter comparison.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ShadowCountReport {
    pub scanned: u64,
    /// Both copies agree.
    pub agreed: u64,
    /// Disagreed on a thread only this account holds — a maintenance
    /// bug. This is the number that has to reach zero before S4.
    pub diverged_single: u64,
    /// Disagreed on a thread more than one local account holds —
    /// expected, and the point of the change.
    pub diverged_shared: u64,
    /// Up to eight `diverged_single` cases, as
    /// `(tid, shared, per_user)` with each triple as
    /// `count/unread/sent`, so a report names rows rather than only
    /// counting them.
    pub samples: Vec<(String, String, String)>,
}

const MAX_SAMPLES: usize = 8;

impl KevyMailboxStore {
    /// Compare both copies over a page of `user`'s threads.
    pub fn shadow_thread_counts(
        &self,
        user: &str,
        offset: i64,
        limit: i64,
    ) -> io::Result<ShadowCountReport> {
        let accounts = self.list_account_addresses().unwrap_or_default();
        if accounts.is_empty() {
            return Err(io::Error::other(
                "no accounts registered — cannot tell a shared thread from a single one, \
                 and the whole point of this report is telling them apart",
            ));
        }

        let prefix = keys::thread_user(user, "");
        let mut ids: Vec<String> = self
            .store()
            .keys(Some(format!("{prefix}*").as_bytes()), None)
            .into_iter()
            .filter_map(|k| {
                let k = String::from_utf8(k).ok()?;
                k.get(prefix.len()..).map(str::to_string)
            })
            .collect();
        ids.sort();

        let mut report = ShadowCountReport::default();
        for tid in ids
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
        {
            report.scanned += 1;
            let Some(row) = self.get_thread(&tid)? else {
                continue;
            };
            let shared = (row.count, row.unread_count, row.sent_count);
            let mine = self.per_user_counts(user, &tid)?;

            if shared == mine {
                report.agreed += 1;
                continue;
            }
            if self.thread_is_shared(&accounts, &tid)? {
                report.diverged_shared += 1;
                continue;
            }
            report.diverged_single += 1;
            if report.samples.len() < MAX_SAMPLES {
                report.samples.push((
                    tid,
                    format!("{}/{}/{}", shared.0, shared.1, shared.2),
                    format!("{}/{}/{}", mine.0, mine.1, mine.2),
                ));
            }
        }
        Ok(report)
    }

    /// `(count, unread_count, sent_count)` off the membership row.
    ///
    /// A missing field reads as zero. That is the same reading
    /// `ThreadRow::from_pairs` gives the shared row, so an absent field
    /// on one side and an explicit zero on the other compare equal
    /// rather than being reported as drift the operator cannot act on.
    fn per_user_counts(&self, user: &str, tid: &str) -> io::Result<(i64, i64, i64)> {
        let key = keys::thread_user(user, tid);
        let read = |field: &[u8]| -> io::Result<i64> {
            Ok(self
                .store()
                .hget(key.as_bytes(), field)?
                .and_then(|v| String::from_utf8(v).ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(0))
        };
        Ok((
            read(b"count")?,
            read(b"unread_count")?,
            read(b"sent_count")?,
        ))
    }

    /// Whether more than one account holds `tid`. One exact lookup per
    /// account, not a keyspace scan.
    ///
    /// `pub` because a rebuild has to ask it too: the shared thread hash
    /// has no user segment, so on a thread two accounts hold it can carry
    /// one of them or the other. A sweep run once may write it and accept
    /// "last one wins"; an operation that walks every account and claims
    /// to settle cannot, because each owner's pass would rewrite it to a
    /// different answer and it would report work forever.
    pub fn thread_is_shared(&self, accounts: &[String], tid: &str) -> io::Result<bool> {
        let mut seen = 0;
        for account in accounts {
            if self
                .store()
                .exists(&[keys::thread_user(account, tid).as_bytes()])?
                > 0
            {
                seen += 1;
                if seen > 1 {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageArrival;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store(accounts: &[&str]) -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        for a in accounts {
            s.upsert_account(a, r#"{"active":true}"#).unwrap();
        }
        s
    }

    fn arrival<'a>(
        tid: &'a str,
        user: &'a str,
        sender: &'a str,
        is_own: bool,
    ) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: sender,
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread: !is_own,
            is_own,
        }
    }

    #[test]
    fn a_thread_one_account_holds_agrees() {
        let s = store(&["u@x.com"]);
        s.record_message_arrival(&arrival("t1", "u@x.com", "alice@y.com", false))
            .unwrap();

        let r = s.shadow_thread_counts("u@x.com", 0, 10).unwrap();
        assert_eq!((r.scanned, r.agreed), (1, 1));
        assert_eq!(r.diverged_single, 0);
        assert_eq!(r.diverged_shared, 0);
    }

    /// The reported bug, seen through the shadow report: the copies
    /// disagree, and the report must say the disagreement is the
    /// expected kind rather than flagging a maintenance failure.
    #[test]
    fn a_thread_two_accounts_hold_diverges_on_purpose() {
        let s = store(&["lihao@x.com", "devops@x.com"]);
        s.record_message_arrival(&arrival("t1", "lihao@x.com", "devops@x.com", false))
            .unwrap();
        s.record_message_arrival(&arrival("t1", "devops@x.com", "devops@x.com", true))
            .unwrap();

        let r = s.shadow_thread_counts("lihao@x.com", 0, 10).unwrap();
        assert_eq!(r.diverged_shared, 1);
        assert_eq!(
            r.diverged_single, 0,
            "a double-counted shared thread is not a maintenance bug"
        );
        assert!(
            r.samples.is_empty(),
            "samples are for actionable cases only"
        );
    }

    /// A writer that updates the shared row and forgets the per-user one
    /// has to land in `diverged_single` and be named.
    #[test]
    fn a_forgotten_writer_is_reported_as_actionable_with_the_row_named() {
        let s = store(&["u@x.com"]);
        s.record_message_arrival(&arrival("t1", "u@x.com", "alice@y.com", false))
            .unwrap();
        // Simulate the shared row moving on its own.
        s.store()
            .hincrby(keys::thread("t1").as_bytes(), b"count", 5)
            .unwrap();

        let r = s.shadow_thread_counts("u@x.com", 0, 10).unwrap();
        assert_eq!(r.diverged_single, 1);
        assert_eq!(r.diverged_shared, 0);
        assert_eq!(r.samples.len(), 1);
        assert_eq!(r.samples[0].0, "t1");
        assert_eq!(r.samples[0].1, "6/1/0", "shared");
        assert_eq!(r.samples[0].2, "1/1/0", "per-user");
    }

    #[test]
    fn a_report_without_accounts_fails_rather_than_calling_everything_single() {
        let s = store(&[]);
        s.record_message_arrival(&arrival("t1", "u@x.com", "alice@y.com", false))
            .unwrap();
        assert!(s.shadow_thread_counts("u@x.com", 0, 10).is_err());
    }
}

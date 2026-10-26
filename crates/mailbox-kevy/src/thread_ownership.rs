//! Who holds a thread.
//!
//! Lived in `shadow_counts.rs` until 2026-08-16, and moved out ahead of
//! that file's deletion because three callers outside it need it — the
//! axis shadow and `reindex` twice. Deleting an index starts with finding
//! the readers of the data; the same care
//! applies to the code: deleting a file takes every function in it,
//! including the ones somebody else depends on, and the compiler tells
//! you only at the call site ("method not found") rather than where it
//! went.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Whether more than one account holds a membership row for `tid`.
    ///
    /// The shared thread hash has no user segment, so on a thread two
    /// accounts hold it can carry one of them or the other. A sweep run
    /// once may write it and accept "last one wins"; an operation that
    /// walks every account and claims to settle cannot, because each
    /// owner's pass would rewrite it to a different answer and it would
    /// report work forever.
    ///
    /// Short-circuits at two: the question is "more than one", not "how
    /// many", and a thread with thirteen owners costs the same as one
    /// with two.
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

    /// Every account's message rows in `tid`, as `(user, message_id)`,
    /// read from the declared `tid` index rather than by walking accounts.
    /// A delete reads it to learn what other owners still hold.
    pub fn thread_message_owners(&self, tid: &str) -> io::Result<Vec<(String, String)>> {
        use kevy_embedded::IndexValue;
        let at = IndexValue::Str(tid.as_bytes().to_vec());
        let mut out = Vec::new();
        let mut cursor = None;
        loop {
            let (rows, next) = self.store().idx_query(
                keys::IDX_USERMSG_BY_TID,
                &at,
                &at,
                cursor.as_ref(),
                10_000,
            )?;
            out.extend(rows.iter().filter_map(|(key, _)| row_owner(key)));
            match next {
                Some(c) => cursor = Some(c),
                None => return Ok(out),
            }
        }
    }

    /// Whether `file` is recorded as one of `user`'s copies of `message_id`.
    pub fn has_message_file(&self, user: &str, message_id: &str, file: &str) -> io::Result<bool> {
        let key = keys::user_message_files(user, message_id);
        Ok(self
            .store()
            .sismember(key.as_bytes(), file_base(file).as_bytes())?)
    }

    /// Record `file` as one of `user`'s copies of `message_id`; whether it
    /// was new. For the backfill — new copies are recorded by
    /// `upsert_user_message`.
    pub fn add_message_file(&self, user: &str, message_id: &str, file: &str) -> io::Result<bool> {
        let key = keys::user_message_files(user, message_id);
        Ok(self
            .store()
            .sadd(key.as_bytes(), &[file_base(file).as_bytes()])?
            > 0)
    }
}

/// `(user, message_id)` from a `mailrs:usermsg:{user}:{message_id}` key.
/// An address has no `:`, so the first one after the prefix ends it; a
/// Message-ID may contain more.
fn row_owner(key: &[u8]) -> Option<(String, String)> {
    let rest = std::str::from_utf8(key.strip_prefix(keys::USER_MESSAGE_PREFIX)?).ok()?;
    let (user, mid) = rest.split_once(':')?;
    Some((user.to_string(), mid.to_string()))
}

/// A `blob_ref` without its `:2,FLAGS`: the name stays true when the
/// message is read and its file renamed.
pub(crate) fn file_base(blob_ref: &str) -> &str {
    blob_ref
        .split_once(":2,")
        .map_or(blob_ref, |(base, _)| base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageArrival;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
        s.ensure_thread_table();
        // The aggregate index that derives the counters, too — without
        // it every count reads zero, which looks exactly like a broken
        // count rather than a store that was never fully booted.
        s.ensure_admin_indexes();
        s
    }

    fn arrive(s: &KevyMailboxStore, tid: &str, user: &str) {
        s.record_message_arrival(&MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "other@z.com",
            latest_date: 100,
            latest_preview: "",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
    }

    /// One owner is not shared; two are. The accounts list is what makes
    /// the question answerable — an empty one reads every thread as
    /// single-owner, which is how a test asserting the multi-owner half
    /// can pass while measuring nothing.
    #[test]
    fn shared_means_more_than_one_account_holds_a_row() {
        let s = store();
        let accounts: Vec<String> = ["a@x.com", "b@x.com", "c@x.com"]
            .iter()
            .map(|a| (*a).to_string())
            .collect();

        arrive(&s, "solo", "a@x.com");
        assert!(!s.thread_is_shared(&accounts, "solo").unwrap());

        arrive(&s, "both", "a@x.com");
        arrive(&s, "both", "b@x.com");
        assert!(s.thread_is_shared(&accounts, "both").unwrap());

        assert!(
            !s.thread_is_shared(&[], "both").unwrap(),
            "with no accounts to ask about, nothing can read as shared"
        );
        assert!(!s.thread_is_shared(&accounts, "never-existed").unwrap());
    }
}

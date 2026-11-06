//! One connected mailbox's page of a list.

use std::io;

use mailrs_mailbox::scope::Scope;

use super::merge::page_of;
use super::{
    ALL_MAIL_SET, BucketSet, ListThreadsFilter, NON_JUNK_SET, NP_SET, QuarantineScope, SentOnly,
    bucket_predicates,
};
use crate::KevyMailboxStore;
use crate::thread_row::ThreadRow;

impl KevyMailboxStore {
    /// One account's page under the rest of the filter.
    ///
    /// Keyed on the account index and given every other predicate as a
    /// value filter, which is the flag axes' shape — so a narrowed
    /// Inbox, or Starred within one mailbox, is still one walk per
    /// bucket and its total is still an index count rather than a page
    /// read twice.
    ///
    /// The account narrows the list and changes nothing else about it:
    /// the scope's buckets and its exclusion of the threads the user only
    /// sent into, the archive, the Trash and held mail are the same
    /// predicates the unnarrowed list asks its own indexes.
    pub(super) fn list_for_account(
        &self,
        user: &str,
        filter: &ListThreadsFilter<'_>,
        want: usize,
        account_id: &str,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let mut extra: Vec<(&str, String)> = Vec::new();
        // the Trash shows what was moved there whether or not it had been
        // archived first; every other list leaves the Trash out
        match filter.trashed {
            true => extra.push(("trashed", "1".to_string())),
            false => {
                extra.push(("trashed", "0".to_string()));
                extra.push(("archived", flag(filter.archived)));
            }
        }
        extra.push((
            "quarantined",
            flag(filter.quarantine == QuarantineScope::Only),
        ));
        for (col, on) in [
            ("starred", filter.starred),
            ("pinned", filter.pinned),
            ("unread", filter.has_unread),
            ("has_action", filter.has_action),
        ] {
            if on {
                extra.push((col, "1".to_string()));
            }
        }
        let walks: Vec<Vec<(&str, String)>> = match filter.scope() {
            Scope::Contradiction => return Ok((Vec::new(), 0)),
            Scope::All => vec![extra],
            Scope::Bucket(b) => vec![bucket_predicates(b.name(), &extra, SentOnly::Excluded)],
            Scope::Category(c) => {
                let mut one = extra;
                one.push(("category", c));
                vec![one]
            }
            Scope::Np => per_bucket(NP_SET, &extra),
            Scope::NotJunk => per_bucket(NON_JUNK_SET, &extra),
            Scope::AllMail | Scope::Unread => per_bucket(ALL_MAIL_SET, &extra),
        };
        let mut merged = Vec::new();
        let mut total = 0usize;
        for walk in walks {
            let refs: Vec<(&str, &str)> = walk.iter().map(|(c, v)| (*c, v.as_str())).collect();
            let n = self.count_thread_ids_by_account(user, account_id, &refs)?;
            total += n;
            if want == 0 {
                continue;
            }
            let tids = self.list_thread_ids_by_account(
                user,
                account_id,
                &refs,
                want,
                0,
                filter.before_ts,
            )?;
            let (rows, _) = self.hydrate_page(user, &tids, n)?;
            merged.extend(rows);
        }
        Ok(page_of(merged, total, 0, want))
    }
}

fn flag(on: bool) -> String {
    match on {
        true => "1".to_string(),
        false => "0".to_string(),
    }
}

/// One walk per bucket of `set`, each scoped the way the set's own read is.
fn per_bucket<'a>(set: BucketSet, extra: &[(&'a str, String)]) -> Vec<Vec<(&'a str, String)>> {
    set.names
        .iter()
        .map(|b| bucket_predicates(b, extra, set.sent))
        .collect()
}

//! The connected-mailbox axis.
//!
//! The same shape as the flag axes next door — key on the column,
//! filter to the user, sort by recency — with a string key instead of
//! `1`. `account_id` is keyed rather than folded into an ORDERPATH
//! because "only these accounts" is N walks: the engine's value
//! filters have `Eq` and no `In`, and N composites for a predicate
//! most people never use would be the wrong trade.
//!
//! This deployment's own mail is the empty string, so it is an account
//! in the filter like any other and can be switched off.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// One page of one account's threads, newest first.
    ///
    /// `extra` carries whatever else the caller is filtering on — the
    /// bucket, the flags — over the columns stored beside this index,
    /// so a narrowed Inbox is still one walk.
    pub fn list_thread_ids_by_account(
        &self,
        user: &str,
        account_id: &str,
        extra: &[(&str, &str)],
        limit: usize,
        offset: usize,
        before_ts: Option<i64>,
    ) -> io::Result<Vec<String>> {
        use kevy_embedded::{IndexValue, ScalarQueryOpts, ValueFilter};
        let (lo, hi);
        let mut filters = vec![ValueFilter::Eq {
            field: b"user",
            value: user.as_bytes(),
        }];
        if let Some(ts) = before_ts {
            lo = i64::MIN.to_string();
            hi = ts.to_string();
            filters.push(ValueFilter::Range {
                field: b"activity",
                min: lo.as_bytes(),
                max: hi.as_bytes(),
            });
        }
        for (col, val) in extra {
            filters.push(ValueFilter::Eq {
                field: col.as_bytes(),
                value: val.as_bytes(),
            });
        }
        let key = IndexValue::Str(account_id.as_bytes().to_vec());
        let page = self.store.idx_query_claused(
            b"threaduser.account_id",
            &key,
            &key,
            None,
            limit,
            ScalarQueryOpts {
                filters: &filters,
                sort: Some((b"activity", true)),
                distinct: None,
                facets: &[],
                offset,
            },
        )?;
        let prefix_len = keys::thread_user(user, "").len();
        Ok(page
            .rows
            .into_iter()
            .filter_map(|(key, _)| {
                let k = String::from_utf8(key).ok()?;
                k.get(prefix_len..).map(str::to_string)
            })
            .collect())
    }

    /// How many threads one account has under the same predicates.
    ///
    /// An index count, not a walk — which is what keeps a filtered
    /// list's total honest without reading the page twice.
    pub fn count_thread_ids_by_account(
        &self,
        user: &str,
        account_id: &str,
        extra: &[(&str, &str)],
    ) -> io::Result<usize> {
        use kevy_embedded::{IndexValue, ValueFilter};
        let mut filters = vec![ValueFilter::Eq {
            field: b"user",
            value: user.as_bytes(),
        }];
        for (col, val) in extra {
            filters.push(ValueFilter::Eq {
                field: col.as_bytes(),
                value: val.as_bytes(),
            });
        }
        let key = IndexValue::Str(account_id.as_bytes().to_vec());
        let n = self
            .store
            .idx_count_claused(b"threaduser.account_id", &key, &key, &filters)?;
        Ok(usize::try_from(n).unwrap_or(usize::MAX))
    }
}

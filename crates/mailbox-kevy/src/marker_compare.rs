//! The flag reads answered off both index sets, for every user, while the
//! full-row indexes are still declared beside the marker indexes.
//!
//! The marker check compares each row with itself; this compares what the
//! reads return, through the same queries the lists and counts use, so a
//! difference in how the two indexes are queried shows up too.

use std::collections::BTreeSet;
use std::io;

use kevy_embedded::IndexValue;

use crate::KevyMailboxStore;
use crate::thread_row::marker_index;

/// The flags the lists and counts key on, each with `[1, 1]`.
const FLAGS: [&str; 7] = [
    "starred",
    "archived",
    "trashed",
    "pinned",
    "unread",
    "has_action",
    "is_sender",
];

/// The timestamps the reads key on with `[1, now]`.
const TIMES: [&str; 3] = ["snoozed_until", "trashed_at", "awaiting_since"];

/// The predicates the views stack on a flag: none, the default views'
/// "not archived, not trashed", and the inbox's unread.
const STACKS: [&[(&str, &str)]; 3] = [
    &[],
    &[("archived", "0"), ("trashed", "0")],
    &[("bucket", "inbox"), ("unread", "1")],
];

/// More than any one user's rows, so a list is never cut short.
const ALL: usize = 1_000_000;

const SAMPLES: usize = 10;

/// What the comparison found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReadCompare {
    pub users: usize,
    /// Reads asked of both index sets.
    pub comparisons: usize,
    /// Rows the marker indexes returned across all of them: zero would
    /// mean nothing was compared.
    pub rows_returned: usize,
    pub differences: usize,
    /// Up to ten of them, as `user flag predicates`.
    pub samples: Vec<String>,
}

impl KevyMailboxStore {
    /// Every flag read, for every user, off the marker index and off the
    /// full-row index; and every timestamp index's keys off both. Read-only.
    /// Meaningful only while the full-row indexes are declared.
    pub fn threaduser_read_compare(&self) -> io::Result<ReadCompare> {
        let mut out = ReadCompare {
            users: 0,
            comparisons: 0,
            rows_returned: 0,
            differences: 0,
            samples: Vec::new(),
        };
        let mut users = BTreeSet::new();
        for key in self.thread_user_keys() {
            if let Some(u) = self.store.hget(&key, b"user")? {
                users.insert(String::from_utf8_lossy(&u).into_owned());
            }
        }
        out.users = users.len();
        let note = |out: &mut ReadCompare, what: String| {
            out.differences += 1;
            if out.samples.len() < SAMPLES {
                out.samples.push(what);
            }
        };
        for user in &users {
            for flag in FLAGS {
                let new = marker_index(flag).expect("a marked flag");
                let old = format!("threaduser.{flag}");
                for extra in STACKS {
                    let list = |ix: &str| {
                        self.list_thread_ids_on_flag_index(ix, user, extra, ALL, 0, None)
                    };
                    let count = |ix: &str| self.count_thread_ids_on_flag_index(ix, user, extra);
                    let (list_new, list_old) = (list(&new)?, list(&old)?);
                    let (count_new, count_old) = (count(&new)?, count(&old)?);
                    out.comparisons += 2;
                    out.rows_returned += list_new.len();
                    if list_new != list_old {
                        note(&mut out, format!("{user} {flag} {extra:?} list"));
                    }
                    if count_new != count_old {
                        note(&mut out, format!("{user} {flag} {extra:?} count"));
                    }
                }
            }
        }
        for col in TIMES {
            let keys_on = |ix: &str| -> io::Result<Vec<Vec<u8>>> {
                let (rows, _) = self.store.idx_query(
                    ix.as_bytes(),
                    &IndexValue::I64(1),
                    &IndexValue::I64(i64::MAX),
                    None,
                    ALL,
                )?;
                let mut keys: Vec<Vec<u8>> = rows.into_iter().map(|(k, _)| k).collect();
                keys.sort();
                Ok(keys)
            };
            let new = keys_on(&marker_index(col).expect("a marked timestamp"))?;
            let old = keys_on(&format!("threaduser.{col}"))?;
            out.comparisons += 1;
            out.rows_returned += new.len();
            if new != old {
                note(&mut out, format!("{col} keys"));
            }
        }
        Ok(out)
    }
}

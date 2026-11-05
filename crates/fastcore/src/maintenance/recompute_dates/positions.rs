//! Where a reader's rows sit in their lists before and after new times.
//!
//! A folder is the prefix of the declared `by_user_bucket_sent` path
//! (bucket, sent_only, archived, quarantined, trashed) and the order under
//! it is `activity DESC, ord`, as the list pages walk it. Pinning, the
//! sectioned inbox and tabs merged in the client are not modelled.

use std::collections::{BTreeMap, HashMap};

use mailrs_mailbox_kevy::{KevyMailboxStore, ListSlot};

/// One changed row's rank (0 = newest) in its folder, before and after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rank {
    pub before: usize,
    pub after: usize,
}

/// The rank of every row in `changes` (thread id → new activity) among all
/// of `user`'s rows, with only those rows' times replaced.
pub(crate) fn ranks(
    mailbox: &KevyMailboxStore,
    user: &str,
    changes: &BTreeMap<String, i64>,
) -> std::io::Result<BTreeMap<String, Rank>> {
    let mut slots: Vec<(String, ListSlot)> = Vec::new();
    for tid in mailbox.all_thread_ids_for_user(user)? {
        if let Some(slot) = mailbox.list_slot(user, &tid)? {
            slots.push((tid, slot));
        }
    }
    let before = rank_all(
        slots
            .iter()
            .map(|(t, s)| (t.as_str(), &s.folder, s.activity, s.ord)),
    );
    let after = rank_all(slots.iter().map(|(t, s)| {
        let activity = changes.get(t).copied().unwrap_or(s.activity);
        (t.as_str(), &s.folder, activity, s.ord)
    }));
    Ok(changes
        .keys()
        .filter_map(|tid| {
            let rank = Rank {
                before: *before.get(tid.as_str())?,
                after: *after.get(tid.as_str())?,
            };
            Some((tid.clone(), rank))
        })
        .collect())
}

fn rank_all<'a>(
    rows: impl Iterator<Item = (&'a str, &'a String, i64, i64)>,
) -> HashMap<&'a str, usize> {
    let mut by_folder: HashMap<&String, Vec<(i64, i64, &str)>> = HashMap::new();
    for (tid, folder, activity, ord) in rows {
        by_folder
            .entry(folder)
            .or_default()
            .push((activity, ord, tid));
    }
    let mut out = HashMap::new();
    for rows in by_folder.values_mut() {
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (i, (_, _, tid)) in rows.iter().enumerate() {
            out.insert(*tid, i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_follow_activity_then_ord_within_each_folder() {
        let inbox = "inbox|0|0|0|0".to_string();
        let sent = "inbox|1|0|0|0".to_string();
        let rows = [
            ("a", &inbox, 300, 2),
            ("b", &inbox, 200, 1),
            ("c", &inbox, 200, 0),
            ("d", &sent, 100, 0),
        ];
        let r = rank_all(rows.into_iter());
        assert_eq!((r["a"], r["c"], r["b"], r["d"]), (0, 1, 2, 0));
    }
}

//! Joining one account's messages into conversations, merging each
//! conversation's fragments, and indexing every message under its thread.

use std::collections::HashMap;

use super::super::prelude::{UnionFind, maildir_references};
use super::Totals;
use super::collect::Msg;
use crate::FastcoreState;

/// Component root → member tids, and → its oldest message's (date, tid).
pub(super) struct Components {
    tids: HashMap<String, Vec<String>>,
    oldest: HashMap<String, (i64, String)>,
}

pub(super) fn link_ancestry(user: &str, msgs: &[Msg], t: &mut Totals) -> UnionFind {
    let mut uf = UnionFind::default();
    // subject lookup so ancestry edges respect the Gmail rule: a
    // reply that changed topic must NOT glue two conversations.
    let subj_by_mid: HashMap<&str, String> = msgs
        .iter()
        .map(|(mid, _, _, _, _, subject)| {
            (
                mid.as_str(),
                mailrs_mailbox_kevy::normalize_subject(subject),
            )
        })
        .collect();
    let subjects_agree = |a: &str, b: &str, subj_by_mid: &HashMap<&str, String>| {
        match (subj_by_mid.get(a), subj_by_mid.get(b)) {
            // unknown side (ancestor never ingested) → trust the edge
            (Some(x), Some(y)) => x == y || x.is_empty() || y.is_empty(),
            _ => true,
        }
    };
    for (mid, irt, _, tid, blob_ref, _) in msgs {
        uf.union(&format!("m:{mid}"), &format!("t:{tid}"));
        if !irt.is_empty() {
            if subjects_agree(mid, irt, &subj_by_mid) {
                uf.union(&format!("m:{mid}"), &format!("m:{irt}"));
                t.in_reply_to_edges += 1;
            } else {
                t.rejected_subject += 1;
            }
        }
        match maildir_references(user, blob_ref) {
            None => {
                t.rejected_unreadable += 1;
                // A count with no example is one step better than silence
                // and still not actionable: prod's first legible run
                // reported 109 unreadable against 31,566 files on disk,
                // so nothing was missing and the cause had to be the
                // reference itself.
                if t.unreadable_samples.len() < 8 {
                    // With the user: the file for every sample was
                    // present on disk under one account, so which
                    // account the row is filed under is the whole
                    // question.
                    t.unreadable_samples.push(format!("{user} {blob_ref}"));
                }
            }
            Some(refs) if refs.is_empty() => t.no_references += 1,
            Some(refs) => {
                for r in refs {
                    if subjects_agree(mid, &r, &subj_by_mid) {
                        uf.union(&format!("m:{mid}"), &format!("m:{r}"));
                        t.reference_edges += 1;
                    } else {
                        t.rejected_subject += 1;
                    }
                }
            }
        }
    }
    uf
}

// component → member tids + its oldest message's tid (canonical)
pub(super) fn components(msgs: &[Msg], uf: &mut UnionFind) -> Components {
    let mut comp_tids: HashMap<String, Vec<String>> = HashMap::new();
    let mut comp_oldest: HashMap<String, (i64, String)> = HashMap::new();
    for (mid, _, date, tid, _, _) in msgs {
        let root = uf.find(&format!("m:{mid}"));
        let entry = comp_tids.entry(root.clone()).or_default();
        if !entry.contains(tid) {
            entry.push(tid.clone());
        }
        let best = comp_oldest.entry(root).or_insert((*date, tid.clone()));
        if *date < best.0 {
            *best = (*date, tid.clone());
        }
    }
    Components {
        tids: comp_tids,
        oldest: comp_oldest,
    }
}

/// Merge every fragment into its component's canonical thread.
///
/// Answers the thread ids that stop existing, and what absorbed them. The
/// Send projection stores a thread id per send, taken at enqueue, and a
/// merge invalidates it — a row left holding a dead id navigates to an
/// empty conversation.
pub(super) fn merge_fragments(
    state: &FastcoreState,
    user: &str,
    comps: &Components,
    t: &mut Totals,
) -> HashMap<String, String> {
    let mut merged_away: HashMap<String, String> = HashMap::new();
    for (root, tids) in &comps.tids {
        let Some((_, canonical)) = comps.oldest.get(root) else {
            continue;
        };
        for tid in tids {
            if tid != canonical {
                match state.mailbox.merge_thread_into(user, tid, canonical) {
                    Ok(n) => {
                        t.merged_threads += 1;
                        t.moved_messages += n as u64;
                        merged_away.insert(tid.clone(), canonical.clone());
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, %user, %tid, %canonical, "merge_thread_into failed");
                    }
                }
            }
        }
    }
    merged_away
}

// The projection lives in network kevy, the threads in the embedded
// store, so this is a cross-store write — done here because the
// merge is the only moment that knows which ids died.
pub(super) fn repoint_sends(
    state: &FastcoreState,
    user: &str,
    merged_away: &HashMap<String, String>,
    t: &mut Totals,
) {
    if merged_away.is_empty() {
        return;
    }
    match state.net_conn() {
        Some(mut conn) => {
            match mailrs_core_sidestate::families::send::repoint_threads(
                &mut conn,
                user,
                merged_away,
            ) {
                Ok(n) => t.sends_repointed += n,
                Err(e) => tracing::warn!(
                    err = %e, %user,
                    "send rows not re-pointed — Send will open an empty thread for them"
                ),
            }
        }
        None => tracing::warn!(
            %user,
            "no network kevy connection — send rows not re-pointed"
        ),
    }
}

// seed the msgid index for every message (merge already
// re-pointed the moved ones; this covers the untouched rest).
pub(super) fn seed_msgid_index(
    state: &FastcoreState,
    user: &str,
    msgs: &[Msg],
    uf: &mut UnionFind,
    comps: &Components,
    t: &mut Totals,
) {
    for (mid, _, _, tid, _, _) in msgs {
        let root = uf.find(&format!("m:{mid}"));
        let Some((_, canonical)) = comps.oldest.get(&root) else {
            continue;
        };
        let target = if comps.tids.get(&root).map(|v| v.len() > 1).unwrap_or(false) {
            canonical
        } else {
            tid
        };
        if state
            .mailbox
            .set_thread_for_message_id(user, mid, target)
            .is_ok()
        {
            t.indexed += 1;
        }
    }
}

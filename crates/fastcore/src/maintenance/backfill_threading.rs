//! Rebuild the msgid → thread index and merge what it joins.
//!
//! One route, and it is the one that reports its own denominator: it
//! answered `msgids_indexed: 9` against a 30,562-row table for however
//! long, because it enumerated a zset that had been dropped.

use super::prelude::*;

/// `POST /v1/admin/backfill-threading` — one-shot rethread of existing
/// data (v2.9.5). Conversations fragmented across multiple thread_ids
/// because three write paths derived roots inconsistently and no msgid
/// index existed. Union-find over (message ↔ its In-Reply-To parent) +
/// (message ↔ its current thread) yields the true conversations; each
/// component's fragments merge into a canonical thread (the one holding
/// the component's oldest message — deterministic, so reruns are
/// idempotent no-ops). Also seeds the msgid→thread index for every
/// message. In-process per `feedback-junk-backfill-oom-finding`.
pub(crate) async fn backfill_threading_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    use mailrs_core_api::method::message::MessageWire;
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let store = state.mailbox.store_ref();
    let mut merged_threads = 0u64;
    let mut moved_messages = 0u64;
    let mut indexed = 0u64;
    let mut sends_repointed = 0u64;
    // What the pass actually looked at. Without these a zero result is
    // ambiguous between "nothing to merge" and "saw nothing" — the reading
    // that cost two failed repair attempts on 2026-07-30, when
    // `msgids_indexed: 9` was the only clue that the enumeration was blind.
    let mut threads_enumerated = 0u64;
    let mut messages_seen = 0u64;
    let mut in_reply_to_edges = 0u64;
    let mut reference_edges = 0u64;
    let mut rejected_subject = 0u64;
    let mut rejected_unreadable = 0u64;
    let mut no_references = 0u64;
    let mut unreadable_samples: Vec<String> = Vec::new();
    // The blast radius of the shared message blob. `mailrs:msg:{mid}` holds
    // six per-user fields — uid, blob_ref, flags, modseq, mailbox_id and
    // user_address itself — while a thread can have several owners, so on a
    // multi-owner thread each of them is whoever wrote last. `keys::thread_user`
    // states the rule that breaks: every per-user fact belongs on a row of
    // its own. It was applied to threads and never to messages.
    let mut distinct_tids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut foreign_user_address = 0u64;
    let mut messages_with_user_address = 0u64;
    for user in &users {
        // collect every (message_id, in_reply_to, internal_date, tid, blob_ref)
        //
        // Enumerated from the declared `threaduser` rows, not from
        // `user_threads_by_activity`. That zset is legacy — it is in
        // `all_user_thread_zsets`, which `drop-legacy-zsets` deletes — so
        // once it had been dropped this loop saw almost nothing and the
        // backfill became a no-op that still answered 200. Measured on prod
        // 2026-07-30: the zset yielded 9 messages while the table held
        // 30,562 thread-user rows, so every References edge and every merge
        // this exists to perform had silently stopped happening.
        //
        // A keyspace scan, which is what the census does for the same rows.
        // Acceptable here and nowhere near a request path: this is a
        // one-shot admin sweep over the whole mailbox by definition.
        let prefix = format!("mailrs:threaduser:{user}:");
        let tids: Vec<Vec<u8>> = store
            .keys(Some(format!("{prefix}*").as_bytes()), None)
            .into_iter()
            .filter_map(|k| {
                let k = String::from_utf8(k).ok()?;
                k.strip_prefix(&prefix).map(|tid| tid.as_bytes().to_vec())
            })
            .collect();
        let tids: Vec<(Vec<u8>, f64)> = tids.into_iter().map(|t| (t, 0.0)).collect();
        threads_enumerated += tids.len() as u64;
        let mut msgs: Vec<(String, String, i64, String, String, String)> = Vec::new();
        let mut senders_by_tid: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        // last INBOUND message per thread — display time/subject must
        // track the other side, not the user's own replies (2026-07-18).
        let mut last_inbound_by_tid: std::collections::HashMap<String, (i64, String)> =
            std::collections::HashMap::new();
        for (tid_b, _) in &tids {
            let Ok(tid) = std::str::from_utf8(tid_b) else {
                continue;
            };
            distinct_tids.insert(tid.to_string());
            for blob in state
                .mailbox
                .thread_messages_for_maintenance(tid)
                .unwrap_or_default()
            {
                if let Ok(w) = serde_json::from_slice::<MessageWire>(&blob) {
                    let ua = w.user_address.as_str();
                    if !ua.is_empty() {
                        messages_with_user_address += 1;
                        if !ua.eq_ignore_ascii_case(user) {
                            foreign_user_address += 1;
                        }
                    }
                    let list = senders_by_tid.entry(tid.to_string()).or_default();
                    let sender = w.sender.trim().to_string();
                    if !sender.is_empty() && !list.iter().any(|s| s.eq_ignore_ascii_case(&sender)) {
                        list.push(sender);
                    }
                    if !mailrs_mailbox_kevy::senders_csv_contains_user(&w.sender, user) {
                        let entry = last_inbound_by_tid
                            .entry(tid.to_string())
                            .or_insert((w.internal_date, w.subject.clone()));
                        if w.internal_date > entry.0 {
                            *entry = (w.internal_date, w.subject.clone());
                        }
                    }
                    // This user's own filename, not the shared blob's —
                    // which names whichever owner wrote last and, for 74
                    // messages on production, a file in another mailbox.
                    // Reading References through it is why
                    // `file_unreadable` sat at 109.
                    let blob_ref = state
                        .mailbox
                        .user_message_facts(user, &w.message_id)
                        .ok()
                        .flatten()
                        .map(|f| f.blob_ref)
                        .unwrap_or(w.blob_ref);
                    msgs.push((
                        w.message_id,
                        w.in_reply_to,
                        w.internal_date,
                        tid.to_string(),
                        blob_ref,
                        w.subject,
                    ));
                }
            }
        }
        // Repair participant unions clobbered by the pre-fix overwrite
        // (a user's own reply used to erase every other participant).
        let mut senders_repaired = 0u64;
        let mut dates_repaired = 0u64;
        for (tid, list) in &senders_by_tid {
            let union = list.join(",");
            if let Ok(Some(mut row)) = state.mailbox.get_thread(tid) {
                let mut dirty = false;
                if row.senders_csv != union && !union.is_empty() {
                    row.senders_csv = union;
                    dirty = true;
                    senders_repaired += 1;
                }
                // own replies used to advance latest_date past the last
                // inbound message — pull the row back to inbound time.
                if let Some((date, subject)) = last_inbound_by_tid.get(tid)
                    && row.latest_date != *date
                {
                    row.latest_date = *date;
                    if !subject.is_empty() {
                        row.subject = subject.clone();
                    }
                    dirty = true;
                    dates_repaired += 1;
                }
                if dirty && state.mailbox.upsert_thread(user, &row).is_err() {
                    tracing::warn!(%user, %tid, "backfill: upsert_thread repair failed");
                }
            }
        }
        if senders_repaired > 0 || dates_repaired > 0 {
            tracing::info!(
                %user,
                senders_repaired,
                dates_repaired,
                "backfill: thread rows repaired"
            );
        }
        if msgs.is_empty() {
            continue;
        }
        // union-find over string nodes: `m:<mid>` and `t:<tid>` — a
        // message unions with its current thread, its In-Reply-To
        // parent, AND every Message-ID in its raw References chain
        // (read from the maildir file — the kevy wire doesn't store the
        // chain). Reply chains stitch fragments together while
        // already-grouped threads never split.
        messages_seen += msgs.len() as u64;
        let mut uf = UnionFind::default();
        // subject lookup so ancestry edges respect the Gmail rule: a
        // reply that changed topic must NOT glue two conversations.
        let subj_by_mid: std::collections::HashMap<&str, String> = msgs
            .iter()
            .map(|(mid, _, _, _, _, subject)| {
                (
                    mid.as_str(),
                    mailrs_mailbox_kevy::normalize_subject(subject),
                )
            })
            .collect();
        let subjects_agree =
            |a: &str, b: &str, subj_by_mid: &std::collections::HashMap<&str, String>| {
                match (subj_by_mid.get(a), subj_by_mid.get(b)) {
                    // unknown side (ancestor never ingested) → trust the edge
                    (Some(x), Some(y)) => x == y || x.is_empty() || y.is_empty(),
                    _ => true,
                }
            };
        for (mid, irt, _, tid, blob_ref, _) in &msgs {
            uf.union(&format!("m:{mid}"), &format!("t:{tid}"));
            if !irt.is_empty() {
                if subjects_agree(mid, irt, &subj_by_mid) {
                    uf.union(&format!("m:{mid}"), &format!("m:{irt}"));
                    in_reply_to_edges += 1;
                } else {
                    rejected_subject += 1;
                }
            }
            match maildir_references(user, blob_ref) {
                None => {
                    rejected_unreadable += 1;
                    // A count with no example is one step better than silence
                    // and still not actionable: prod's first legible run
                    // reported 109 unreadable against 31,566 files on disk,
                    // so nothing was missing and the cause had to be the
                    // reference itself.
                    if unreadable_samples.len() < 8 {
                        // With the user: the file for every sample was
                        // present on disk under one account, so which
                        // account the row is filed under is the whole
                        // question.
                        unreadable_samples.push(format!("{user} {blob_ref}"));
                    }
                }
                Some(refs) if refs.is_empty() => no_references += 1,
                Some(refs) => {
                    for r in refs {
                        if subjects_agree(mid, &r, &subj_by_mid) {
                            uf.union(&format!("m:{mid}"), &format!("m:{r}"));
                            reference_edges += 1;
                        } else {
                            rejected_subject += 1;
                        }
                    }
                }
            }
        }
        // component → member tids + its oldest message's tid (canonical)
        let mut comp_tids: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut comp_oldest: std::collections::HashMap<String, (i64, String)> =
            std::collections::HashMap::new();
        for (mid, _, date, tid, _, _) in &msgs {
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
        // Thread ids that stop existing, and what absorbed them. The Send
        // projection stores a thread id per send, taken at enqueue, and a
        // merge invalidates it — a row left holding a dead id navigates to
        // an empty conversation.
        let mut merged_away: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (root, tids) in &comp_tids {
            let Some((_, canonical)) = comp_oldest.get(root) else {
                continue;
            };
            for tid in tids {
                if tid != canonical {
                    match state.mailbox.merge_thread_into(user, tid, canonical) {
                        Ok(n) => {
                            merged_threads += 1;
                            moved_messages += n as u64;
                            merged_away.insert(tid.clone(), canonical.clone());
                        }
                        Err(e) => {
                            tracing::warn!(err = %e, %user, %tid, %canonical, "merge_thread_into failed");
                        }
                    }
                }
            }
        }
        // The projection lives in network kevy, the threads in the embedded
        // store, so this is a cross-store write — done here because the
        // merge is the only moment that knows which ids died.
        if !merged_away.is_empty() {
            match state.net_conn() {
                Some(mut conn) => {
                    match mailrs_core_sidestate::families::send::repoint_threads(
                        &mut conn,
                        user,
                        &merged_away,
                    ) {
                        Ok(n) => sends_repointed += n,
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
        for (mid, _, _, tid, _, _) in &msgs {
            let root = uf.find(&format!("m:{mid}"));
            let Some((_, canonical)) = comp_oldest.get(&root) else {
                continue;
            };
            let target = if comp_tids.get(&root).map(|v| v.len() > 1).unwrap_or(false) {
                canonical
            } else {
                tid
            };
            if state
                .mailbox
                .set_thread_for_message_id(user, mid, target)
                .is_ok()
            {
                indexed += 1;
            }
        }
    }
    tracing::info!(
        merged_threads,
        moved_messages,
        indexed,
        "backfill-threading complete"
    );
    Json(serde_json::json!({
        // What changed.
        "merged_threads": merged_threads,
        "moved_messages": moved_messages,
        "msgids_indexed": indexed,
        "sends_repointed": sends_repointed,
        // What was looked at, so a row of zeros above is legible. All zero
        // here means the enumeration is blind, which is a different fault
        // from having nothing to do.
        "threads_enumerated": threads_enumerated,
        // Rows minus distinct threads = threads with more than one owner,
        // which is where every per-user field on the shared message blob is
        // whoever wrote last.
        "distinct_threads": distinct_tids.len(),
        "messages_with_user_address": messages_with_user_address,
        // Read by a user the blob does not name. Each is a message whose
        // uid, flags, modseq and blob_ref belong to someone else.
        "foreign_user_address": foreign_user_address,
        "messages_seen": messages_seen,
        // Which ancestry edges were found, and what was turned down.
        "in_reply_to_edges": in_reply_to_edges,
        "reference_edges": reference_edges,
        "edges_rejected": {
            // A reply that changed topic: the Gmail rule declining to glue
            // two conversations. Expected to be non-zero.
            "subject_mismatch": rejected_subject,
            // The maildir file could not be opened. Any number here is a
            // defect — it means References were unreadable, not absent.
            "file_unreadable": rejected_unreadable,
            "file_unreadable_samples": unreadable_samples,
        },
        // Read fine, names no ancestor. Not a rejection: most mail is not a
        // reply.
        "messages_without_references": no_references,
    }))
    .into_response()
}

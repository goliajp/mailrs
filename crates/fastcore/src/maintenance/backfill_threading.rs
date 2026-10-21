//! Rebuild the msgid → thread index and merge what it joins.
//!
//! One route, and it is the one that reports its own denominator: it
//! answered `msgids_indexed: 9` against a 30,562-row table for however
//! long, because it enumerated a zset that had been dropped.

use super::prelude::*;

mod collect;
mod merge;

/// Everything the pass counts, across every account.
#[derive(Default)]
struct Totals {
    merged_threads: u64,
    moved_messages: u64,
    indexed: u64,
    sends_repointed: u64,
    // What the pass actually looked at. Without these a zero result is
    // ambiguous between "nothing to merge" and "saw nothing" — the reading
    // that cost two failed repair attempts on 2026-07-30, when
    // `msgids_indexed: 9` was the only clue that the enumeration was blind.
    threads_enumerated: u64,
    messages_seen: u64,
    in_reply_to_edges: u64,
    reference_edges: u64,
    rejected_subject: u64,
    rejected_unreadable: u64,
    no_references: u64,
    unreadable_samples: Vec<String>,
    // The blast radius of the shared message blob. `mailrs:msg:{mid}` holds
    // six per-user fields — uid, blob_ref, flags, modseq, mailbox_id and
    // user_address itself — while a thread can have several owners, so on a
    // multi-owner thread each of them is whoever wrote last. `keys::thread_user`
    // states the rule that breaks: every per-user fact belongs on a row of
    // its own. It was applied to threads and never to messages.
    distinct_tids: std::collections::HashSet<String>,
    foreign_user_address: u64,
    messages_with_user_address: u64,
}

/// `POST /v1/admin/backfill-threading` — one-shot rethread of existing
/// data (v2.9.5). Conversations fragmented across multiple thread_ids
/// because three write paths derived roots inconsistently and no msgid
/// index existed. Union-find over (message ↔ its In-Reply-To parent) +
/// (message ↔ its current thread) yields the true conversations; each
/// component's fragments merge into a canonical thread (the one holding
/// the component's oldest message — deterministic, so reruns are
/// idempotent no-ops). Also seeds the msgid→thread index for every
/// message. In-process for the same reason as the admin-key sweep: a
/// separate binary would double-open the embedded kevy.
pub(crate) async fn backfill_threading_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut t = Totals::default();
    for user in &users {
        rethread_user(&state, user, &mut t);
    }
    tracing::info!(
        merged_threads = t.merged_threads,
        moved_messages = t.moved_messages,
        indexed = t.indexed,
        "backfill-threading complete"
    );
    Json(report(t)).into_response()
}

fn rethread_user(state: &FastcoreState, user: &str, t: &mut Totals) {
    let found = collect::user_messages(state, user, t);
    collect::repair_thread_rows(state, user, &found);
    let msgs = found.msgs;
    if msgs.is_empty() {
        return;
    }
    // union-find over string nodes: `m:<mid>` and `t:<tid>` — a
    // message unions with its current thread, its In-Reply-To
    // parent, AND every Message-ID in its raw References chain
    // (read from the maildir file — the kevy wire doesn't store the
    // chain). Reply chains stitch fragments together while
    // already-grouped threads never split.
    t.messages_seen += msgs.len() as u64;
    let mut uf = merge::link_ancestry(user, &msgs, t);
    let comps = merge::components(&msgs, &mut uf);
    let merged_away = merge::merge_fragments(state, user, &comps, t);
    merge::repoint_sends(state, user, &merged_away, t);
    merge::seed_msgid_index(state, user, &msgs, &mut uf, &comps, t);
}

fn report(t: Totals) -> serde_json::Value {
    serde_json::json!({
        // What changed.
        "merged_threads": t.merged_threads,
        "moved_messages": t.moved_messages,
        "msgids_indexed": t.indexed,
        "sends_repointed": t.sends_repointed,
        // What was looked at, so a row of zeros above is legible. All zero
        // here means the enumeration is blind, which is a different fault
        // from having nothing to do.
        "threads_enumerated": t.threads_enumerated,
        // Rows minus distinct threads = threads with more than one owner,
        // which is where every per-user field on the shared message blob is
        // whoever wrote last.
        "distinct_threads": t.distinct_tids.len(),
        "messages_with_user_address": t.messages_with_user_address,
        // Read by a user the blob does not name. Each is a message whose
        // uid, flags, modseq and blob_ref belong to someone else.
        "foreign_user_address": t.foreign_user_address,
        "messages_seen": t.messages_seen,
        // Which ancestry edges were found, and what was turned down.
        "in_reply_to_edges": t.in_reply_to_edges,
        "reference_edges": t.reference_edges,
        "edges_rejected": {
            // A reply that changed topic: the Gmail rule declining to glue
            // two conversations. Expected to be non-zero.
            "subject_mismatch": t.rejected_subject,
            // The maildir file could not be opened. Any number here is a
            // defect — it means References were unreadable, not absent.
            "file_unreadable": t.rejected_unreadable,
            "file_unreadable_samples": t.unreadable_samples,
        },
        // Read fine, names no ancestor. Not a rejection: most mail is not a
        // reply.
        "messages_without_references": t.no_references,
    })
}

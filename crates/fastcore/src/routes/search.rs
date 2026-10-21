//! Conversation search: the text indexes, then a substring scan.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::*;

/// `POST /v1/users/{user}/conversations:search` — ranked full-text
/// lookup over the caller's threads.
///
/// Served by the kevy text index declared in `ensure_admin_indexes`,
/// which kevy maintains from its commit hook — the index cannot lag the
/// rows, unlike the external search service this replaced.
pub(crate) async fn search_conversations(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::SearchConversationsRequest>,
) -> Json<conv::SearchConversationsResponse> {
    let limit = if req.limit == 0 {
        20
    } else {
        req.limit as usize
    };
    // Four stages, in the order a person would look: the words you
    // typed, then those words inside a message, then inside one of its
    // PDF attachments, then anything merely containing them. Each stage skips what the ones before it already
    // returned — a token match is a substring match too, so without the
    // skip the last stage would repeat the first in full.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut tids: Vec<String> = Vec::new();
    let push =
        |tid: String, tids: &mut Vec<String>, seen: &mut std::collections::HashSet<String>| {
            if seen.insert(tid.clone()) {
                tids.push(tid);
            }
        };

    // Overfetch: the scope filter below runs after the index has chosen
    // its page, so a query whose matches are mostly in other folders
    // needs more than `limit` candidates to fill one.
    const SCOPE_OVERFETCH: usize = 8;
    let want = limit.saturating_mul(SCOPE_OVERFETCH);

    for (tid, _) in state
        .mailbox
        .search_threads(&user, &req.query, want)
        .unwrap_or_default()
    {
        push(tid, &mut tids, &mut seen);
    }
    if let Ok(body_hits) = state.mailbox.search_message_bodies(&user, &req.query, want) {
        for tid in body_hits {
            push(tid, &mut tids, &mut seen);
        }
    }
    if let Ok(attachment_hits) = state
        .mailbox
        .search_attachment_texts(&user, &req.query, want)
    {
        for tid in attachment_hits {
            push(tid, &mut tids, &mut seen);
        }
    }
    if let Ok(like_hits) = state.mailbox.like_threads(&user, &req.query, &seen, want) {
        for tid in like_hits {
            push(tid, &mut tids, &mut seen);
        }
    }

    // The searcher's own copy of each hit. The shared hash has no user
    // segment, so reading it here handed one owner's counters, flags and
    // category to the other — the same defect the list had.
    let items = tids
        .into_iter()
        .filter_map(|tid| {
            state
                .mailbox
                .get_thread_for_user(&user, &tid)
                .ok()
                .flatten()
        })
        .filter(|row| in_search_scope(row, &req))
        .take(limit)
        .map(row_to_wire)
        .collect();
    Json(conv::SearchConversationsResponse { items })
}

/// Whether a hit belongs to the tab the search was issued from.
///
/// Deliberately the row's own fields: the membership row is where every
/// per-user fact lives, and reading the tab's predicates from anywhere
/// else is how the list and its counters came apart before.
fn in_search_scope(
    row: &mailrs_mailbox_kevy::ThreadRow,
    req: &conv::SearchConversationsRequest,
) -> bool {
    use mailrs_mailbox_kevy::scope::{ThreadFacts, is_sent_folder, scope};
    if row.archived != req.archived {
        return false;
    }
    // Search is a list, and held mail is out of every list. Without
    // this a reader could reach a conversation by searching that the
    // Inbox, Junk and every other tab agree not to show — and the
    // one thing a hold has to be is consistent, or it reads as mail
    // that comes and goes.
    if row.quarantined {
        return false;
    }
    if req.unread == Some(true) && row.unread_count == 0 {
        return false;
    }
    if req.starred == Some(true) && !row.starred {
        return false;
    }
    let folder = req.folder.as_deref();
    if is_sent_folder(folder) && row.sent_count == 0 {
        return false;
    }
    // The same scope the list answers for this folder and category.
    scope(folder, req.category.as_deref()).admits(ThreadFacts {
        category: &row.category,
        sent_only: row.count > 0 && row.sent_count >= row.count,
    })
}

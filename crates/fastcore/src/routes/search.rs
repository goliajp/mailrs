//! Conversation search: the text indexes, then a substring scan.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use mailrs_mailbox_kevy::query::{Cond, MessageFacts, Place, Query, ThreadFacts, Within};

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
    let query = Query::parse(&req.query);
    let rows = candidates(&state, &user, &query, limit);
    let names = label_names(&state, &user, &query);
    let items = rows
        .into_iter()
        .filter(|row| in_place(row, &req, query.within))
        .filter(|row| {
            let labels = names.get(&row.thread_id).cloned().unwrap_or_default();
            query.admits(&thread_facts(row, &labels), || {
                message_facts(&state, &user, &row.thread_id)
            })
        })
        .take(limit)
        .map(row_to_wire)
        .collect();
    let items = crate::routes::labels::with_labels(&state, &user, items);
    Json(conv::SearchConversationsResponse { items })
}

/// The searcher's own rows to judge, best first.
///
/// With words, the text indexes choose them; with operators only, every
/// conversation the user has, newest first, since nothing narrower can
/// answer `to:` or `has:attachment`.
fn candidates(
    state: &FastcoreState,
    user: &str,
    query: &Query,
    limit: usize,
) -> Vec<mailrs_mailbox_kevy::ThreadRow> {
    if !query.text.is_empty() {
        return text_hits(state, user, &query.text, limit)
            .into_iter()
            .filter_map(|tid| state.mailbox.get_thread_for_user(user, &tid).ok().flatten())
            .collect();
    }
    if !query.has_operators() {
        return Vec::new();
    }
    let mut rows: Vec<_> = state
        .mailbox
        .all_thread_ids_for_user(user)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tid| state.mailbox.get_thread_for_user(user, &tid).ok().flatten())
        .collect();
    rows.sort_by(|a, b| {
        b.latest_date
            .cmp(&a.latest_date)
            .then_with(|| a.thread_id.cmp(&b.thread_id))
    });
    rows
}

/// Thread ids matching `text`, best first.
fn text_hits(state: &FastcoreState, user: &str, text: &str, limit: usize) -> Vec<String> {
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

    // Overfetch: the scope filter runs after the index has chosen its
    // page, so a query whose matches are mostly in other folders needs
    // more than `limit` candidates to fill one.
    const SCOPE_OVERFETCH: usize = 8;
    let want = limit.saturating_mul(SCOPE_OVERFETCH);

    for (tid, _) in state
        .mailbox
        .search_threads(user, text, want)
        .unwrap_or_default()
    {
        push(tid, &mut tids, &mut seen);
    }
    if let Ok(body_hits) = state.mailbox.search_message_bodies(user, text, want) {
        for tid in body_hits {
            push(tid, &mut tids, &mut seen);
        }
    }
    if let Ok(attachment_hits) = state.mailbox.search_attachment_texts(user, text, want) {
        for tid in attachment_hits {
            push(tid, &mut tids, &mut seen);
        }
    }
    if let Ok(like_hits) = state.mailbox.like_threads(user, text, &seen, want) {
        for tid in like_hits {
            push(tid, &mut tids, &mut seen);
        }
    }
    tids
}

/// The names of the labels on each conversation, read only when the
/// query asks about labels.
fn label_names(
    state: &FastcoreState,
    user: &str,
    query: &Query,
) -> std::collections::HashMap<String, Vec<String>> {
    let asks = query.conds.iter().any(|(_, c)| matches!(c, Cond::Label(_)));
    if !asks {
        return Default::default();
    }
    let defs: std::collections::HashMap<String, String> = state
        .mailbox
        .labels(user)
        .unwrap_or_default()
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect();
    state
        .mailbox
        .labels_by_thread(user)
        .unwrap_or_default()
        .into_iter()
        .map(|(tid, ids)| {
            let names = ids.iter().filter_map(|id| defs.get(id).cloned()).collect();
            (tid, names)
        })
        .collect()
}

fn thread_facts<'a>(
    row: &'a mailrs_mailbox_kevy::ThreadRow,
    labels: &'a [String],
) -> ThreadFacts<'a> {
    ThreadFacts {
        subject: &row.subject,
        senders: &row.senders_csv,
        latest_date: row.latest_date,
        unread_count: row.unread_count,
        starred: row.starred,
        pinned: row.pinned,
        labels,
    }
}

/// This user's copy of each message in the thread, as the operators read it.
pub(crate) fn message_facts(
    state: &FastcoreState,
    user: &str,
    thread_id: &str,
) -> Vec<MessageFacts> {
    state
        .mailbox
        .list_thread_messages(user, thread_id)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|b| {
            serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&b).ok()
        })
        .map(|w| MessageFacts {
            sender: w.sender,
            recipients: w.recipients,
            subject: w.subject,
            attachments: w.attachments,
        })
        .collect()
}

/// Whether a conversation is where the search looks: the place `in:`
/// names, or else the tab the search was made from. Held mail is in
/// neither.
fn in_place(
    row: &mailrs_mailbox_kevy::ThreadRow,
    req: &conv::SearchConversationsRequest,
    within: Option<Within>,
) -> bool {
    if row.quarantined {
        return false;
    }
    let Some(within) = within else {
        return in_search_scope(row, req);
    };
    within.admits(Place {
        archived: row.archived,
        trashed: row.trashed_at > 0,
        junk: mailrs_mailbox_kevy::scope::bucket_of(&row.category)
            == mailrs_mailbox_kevy::scope::Bucket::Junk,
        has_sent: row.sent_count > 0,
        sent_only: row.count > 0 && row.sent_count >= row.count,
        starred: row.starred,
    })
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
    if row.archived != req.archived || row.trashed_at > 0 {
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

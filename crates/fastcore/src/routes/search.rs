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
    let limit = req.limit();
    let query = Query::parse(&req.query);
    let listed = match query.within {
        None => listed(&state, &user, &req),
        // `in:` names the place, whatever list the search came from
        Some(_) => Listed::Folder,
    };
    let rows = candidates(&state, &user, &query, req.text_hits_wanted(), &listed);
    let names = label_names(&state, &user, &query);
    let items = rows
        .into_iter()
        .filter(|row| in_place(row, &req, query.within, &listed))
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
    let items = crate::sections::with_sections(&state, &user, items);
    let items = crate::shared::with_shared(&state, &user, items);
    Json(conv::SearchConversationsResponse { items })
}

/// The conversations the list a search was issued from shows.
enum Listed {
    /// A list read off an index of its own, read whole: exactly these.
    Ids(std::collections::HashSet<String>),
    /// A folder's list — every conversation the folder holds, too many to
    /// read for one search — judged row by row by [`in_folder_list`].
    Folder,
}

/// The list `req` was issued from, asked the question the list itself
/// asks, so the search inside it cannot select different conversations.
fn listed(state: &FastcoreState, user: &str, req: &conv::SearchConversationsRequest) -> Listed {
    use mailrs_core_api::method::conversation::LIST_SCOPE_CAP;
    if !req.reads_a_list_of_its_own() {
        return Listed::Folder;
    }
    let rows = match req.quarantined {
        // the Review list, as `list_quarantined` reads it
        true => {
            let filter = ListThreadsFilter {
                quarantine: QuarantineScope::Only,
                ..Default::default()
            };
            state
                .mailbox
                .list_threads_by_activity(user, &filter, 0, LIST_SCOPE_CAP)
                .map(|(rows, _)| rows)
        }
        false => {
            let whole = mailrs_core_api::types::ConversationFilter {
                before_ts: None,
                after_cursor: None,
                ..req.filter.clone()
            };
            crate::routes::list_rows(state, user, &whole, LIST_SCOPE_CAP)
        }
    };
    let rows = rows.unwrap_or_else(|e| {
        tracing::warn!(%user, error = %e, "search: the list it scopes to failed; finding nothing");
        Vec::new()
    });
    Listed::Ids(rows.into_iter().map(|r| r.thread_id).collect())
}

/// The searcher's own rows to judge, best first.
///
/// With words, the text indexes choose them; with operators only, every
/// conversation the user has, newest first, since nothing narrower can
/// answer `to:` or `has:attachment`. Ids outside a list read whole are
/// dropped before their rows are read.
fn candidates(
    state: &FastcoreState,
    user: &str,
    query: &Query,
    want: usize,
    listed: &Listed,
) -> Vec<mailrs_mailbox_kevy::ThreadRow> {
    let in_list = |tid: &String| match listed {
        Listed::Ids(ids) => ids.contains(tid),
        Listed::Folder => true,
    };
    if !query.text.is_empty() {
        return text_hits(state, user, &query.text, want)
            .into_iter()
            .filter(in_list)
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
        .filter(in_list)
        .filter_map(|tid| state.mailbox.get_thread_for_user(user, &tid).ok().flatten())
        .collect();
    rows.sort_by(|a, b| {
        b.latest_date
            .cmp(&a.latest_date)
            .then_with(|| a.thread_id.cmp(&b.thread_id))
    });
    rows
}

/// Thread ids matching `text`, best first, at most `want` from each stage.
///
/// The scope runs after the index has chosen, so `want` is more than a
/// page: see [`conv::SearchConversationsRequest::text_hits_wanted`].
fn text_hits(state: &FastcoreState, user: &str, text: &str, want: usize) -> Vec<String> {
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
/// names, where held mail never is, or else the list the search was
/// issued from.
fn in_place(
    row: &mailrs_mailbox_kevy::ThreadRow,
    req: &conv::SearchConversationsRequest,
    within: Option<Within>,
    listed: &Listed,
) -> bool {
    let Some(within) = within else {
        return match listed {
            Listed::Ids(ids) => ids.contains(&row.thread_id),
            Listed::Folder => in_folder_list(row, &req.filter),
        };
    };
    if row.quarantined {
        return false;
    }
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

/// Whether a folder's list `f` shows this row, predicate for predicate
/// what [`threads_filter`] asks the index.
///
/// Deliberately the row's own fields: the membership row is where every
/// per-user fact lives, and reading the tab's predicates from anywhere
/// else is how the list and its counters came apart before.
fn in_folder_list(
    row: &mailrs_mailbox_kevy::ThreadRow,
    f: &mailrs_core_api::types::ConversationFilter,
) -> bool {
    use mailrs_mailbox_kevy::scope::{Scope, ThreadFacts, is_sent_folder, scope};
    // Archived is a tab across folders, not a predicate inside one
    let folder = match f.archived {
        true => None,
        false => f.folder.as_deref(),
    };
    let scope = scope(folder, f.category.as_deref());
    // All Mail reads both sides of the archive
    if scope != Scope::AllMail && row.archived != f.archived {
        return false;
    }
    if row.trashed_at > 0 {
        return false;
    }
    // Held mail is out of every list unless the reader shows it in place;
    // without this a search could reach a conversation the list it was
    // typed into agrees not to show.
    if row.quarantined && !f.include_quarantined {
        return false;
    }
    if f.unread == Some(true) && row.unread_count == 0 {
        return false;
    }
    if f.starred == Some(true) && !row.starred {
        return false;
    }
    if f.accounts
        .as_ref()
        .is_some_and(|ids| !ids.contains(&row.account_id))
    {
        return false;
    }
    if is_sent_folder(folder) && row.sent_count == 0 {
        return false;
    }
    scope.admits(ThreadFacts {
        category: &row.category,
        sent_only: row.count > 0 && row.sent_count >= row.count,
    })
}

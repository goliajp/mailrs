//! The read side: conversation lists, search, counts, thread contents.
//!
//! Each of these reads **this user's** membership row, not the shared
//! thread hash. The shared one has no user segment, so on a thread two
//! accounts both received it holds whichever owner wrote last — see
//! `.claude/rules/measure-before-you-cut-over.md`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::*;

/// `POST /v1/users/{user}/conversations:list`.
pub(crate) async fn list_conversations(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ListConversationsRequest>,
) -> Json<conv::ListConversationsResponse> {
    let f = &req.filter;
    // Archived is a tab, not a predicate inside the current folder —
    // the UI's own tab resolver returns 'archived' ahead of the folder
    // it was opened from. The client keeps sending that folder, so
    // honouring both answers "archived within Inbox", which is 0 for a
    // thread filed under notifications and is not what the tab means.
    let folder = if f.archived {
        None
    } else {
        f.folder.as_deref()
    };
    let filter = ListThreadsFilter {
        category: f.category.as_deref(),
        folder,
        pinned: false,
        archived: f.archived,
        has_unread: f.unread.unwrap_or(false),
        has_action: false,
        starred: f.starred.unwrap_or(false),
        before_ts: f.before_ts,
    };
    let limit = if f.limit == 0 { 50 } else { f.limit as usize };
    // An error here reads as an empty mailbox, which is the one answer
    // the caller cannot tell from a real one — so say it happened. The
    // dispatcher returns Err when a query names a column its index does
    // not store, and that is a wiring mistake, not an empty page.
    let (rows, _total) = state
        .mailbox
        .list_threads_by_activity(&user, &filter, 0, limit)
        .unwrap_or_else(|e| {
            tracing::warn!(%user, error = %e, "conversation list failed; serving empty");
            (Vec::new(), 0)
        });

    let items = rows.into_iter().map(row_to_wire).collect();
    Json(conv::ListConversationsResponse { items })
}

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
    // Three stages, in the order a person would look: the words you
    // typed, then those words inside a message, then anything merely
    // containing them. Each stage skips what the ones before it already
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
    if let Some(c) = &req.category
        && &row.category != c
    {
        return false;
    }
    if row.archived != req.archived {
        return false;
    }
    if req.unread == Some(true) && row.unread_count == 0 {
        return false;
    }
    if req.starred == Some(true) && !row.starred {
        return false;
    }
    let Some(folder) = req.folder.as_deref() else {
        return true;
    };
    let bucket = mailrs_mailbox_kevy::keys::bucket_of(&row.category);
    let sent_only = row.count > 0 && row.sent_count >= row.count;
    match folder {
        f if f.eq_ignore_ascii_case("inbox") => {
            bucket == mailrs_mailbox_kevy::keys::Bucket::Inbox && !sent_only
        }
        f if f.eq_ignore_ascii_case("junk") => bucket == mailrs_mailbox_kevy::keys::Bucket::Junk,
        f if f.eq_ignore_ascii_case("notifications") => {
            bucket == mailrs_mailbox_kevy::keys::Bucket::Notifications
        }
        f if f.eq_ignore_ascii_case("promotions") => {
            bucket == mailrs_mailbox_kevy::keys::Bucket::Promotions
        }
        f if f.eq_ignore_ascii_case("np") => matches!(
            bucket,
            mailrs_mailbox_kevy::keys::Bucket::Notifications
                | mailrs_mailbox_kevy::keys::Bucket::Promotions
        ),
        f if f.eq_ignore_ascii_case("nonjunk") => bucket != mailrs_mailbox_kevy::keys::Bucket::Junk,
        f if f.eq_ignore_ascii_case("sent") => row.sent_count > 0,
        // An unknown folder is no scope, the same reading
        // `ListThreadsFilter::scope` gives it.
        _ => true,
    }
}

/// `GET /v1/users/{user}/conversations/categories` — histogram of
/// category → distinct thread_id count, read straight off the per-
/// category zsets.
pub(crate) async fn get_categories(
    State(state): State<Arc<FastcoreState>>,
    Path(_user): Path<String>,
) -> Json<conv::ConversationCategoriesResponse> {
    // The vocabulary is `mailrs_triage::CLASSES` and nothing else —
    // the ingest path stamps whatever `classify_triage` returns, and
    // that is the same constant. Reading it here is what stops the two
    // drifting: this list used to be hand-written and said
    // `promotions` / `notifications` (the *bucket* names, plural)
    // where the stored `category` is singular, so every count but
    // `inbox` came back 0 and the dashboard read "Inbox 100%" against
    // a mailbox that was three-quarters notifications and promotions.
    //
    // `spam` / `scam` deliberately absent (user directive 2026-07-13
    // "我希望只有 junk"): those threads live in the Junk FOLDER — the
    // sidebar's Junk entry is their one and only surface. Exposing
    // them as Inbox category tabs double-listed junk mail inside the
    // Inbox view.
    let categories: Vec<conv::CategoryCount> = mailrs_triage::CLASSES
        .into_iter()
        .map(|cat| conv::CategoryCount {
            category: cat.to_string(),
            count: state
                .mailbox
                .count_thread_ids_by_category_via_table(&_user, cat)
                .unwrap_or(0) as i64,
        })
        .filter(|c| c.count > 0)
        .collect();
    Json(conv::ConversationCategoriesResponse { categories })
}

/// `GET /v1/users/{user}/conversations/unseen-count` — a count on the
/// unread axis.
pub(crate) async fn get_unseen_count(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<conv::UnseenCountResponse> {
    // Counted over the same buckets the Unread view lists, and for the
    // same reason: a badge the user cannot click through to is worse
    // than no badge. It read every bucket including Junk until
    // 2026-08-03, while every view it linked to was Inbox-scoped — two
    // unread promotions showed as "2 Unread" on the dashboard and as
    // an empty list everywhere that number led.
    let count = state
        .mailbox
        .count_flag_non_junk(&user, "unread")
        .unwrap_or(0) as i64;
    Json(conv::UnseenCountResponse { count })
}

/// `GET /v1/users/{user}/threads/{thread_id}/messages` — returns the
/// thread's messages. mailbox-kevy doesn't store per-message rows yet
/// (only the aggregate row), so this returns the empty list until
/// Phase 7.11 lands a per-message layout. Webapi treats empty as
/// "thread exists but currently rendering, retry shortly" — graceful
/// fallback that keeps the UI from 404-ing the whole conversation
/// view while the kevy data shape grows.
pub(crate) async fn thread_messages(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Json<mailrs_core_api::method::thread::ListThreadMessagesResponse> {
    use mailrs_core_api::method::message::MessageWire;
    let blobs = state
        .mailbox
        .list_thread_messages(&user, &thread_id)
        .unwrap_or_default();
    let items = blobs
        .into_iter()
        .filter_map(|b| serde_json::from_slice::<MessageWire>(&b).ok())
        .collect();
    Json(mailrs_core_api::method::thread::ListThreadMessagesResponse { items })
}

pub(crate) async fn list_sent_messages(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<mailrs_core_api::method::thread::SentMessagesResponse> {
    use mailrs_core_api::method::message::MessageWire;
    use mailrs_core_api::method::thread::{SentMessageSummary, SentMessagesResponse};

    let tids = sent_thread_ids(&state, &user);

    let mut items: Vec<SentMessageSummary> = Vec::new();
    for tid in &tids {
        let tid = tid.as_str();
        let blobs = state
            .mailbox
            .list_thread_messages(&user, tid)
            .unwrap_or_default();
        for b in blobs {
            let Ok(w) = serde_json::from_slice::<MessageWire>(&b) else {
                continue;
            };
            if !mailrs_mailbox_kevy::senders_csv_contains_user(&w.sender, &user) {
                continue;
            }
            items.push(SentMessageSummary {
                uid: w.uid,
                message_id: w.message_id,
                // the thread this message is actually indexed under (the
                // merged conversation), NOT w.thread_id — a reply's stored
                // thread_id can be its own message-id-based self-thread,
                // which opens an isolated 1-message view. `tid` is what the
                // frontend resolves via get_thread_messages.
                thread_id: tid.to_string(),
                to: w.recipients,
                subject: w.subject,
                internal_date: w.internal_date,
            });
        }
    }
    items.sort_by_key(|s| std::cmp::Reverse(s.internal_date));
    Json(SentMessagesResponse { items })
}

/// `GET /v1/users/{user}/threads/by-message-id/{message_id}` — resolve a
/// RFC 5322 Message-ID to the thread it was indexed under (the msgid →
/// thread reconciliation index). Callers: webapi mirror_send, so a sent
/// reply joins the conversation its parent lives in.
pub(crate) async fn find_thread_by_message_id(
    State(state): State<Arc<FastcoreState>>,
    Path((user, message_id)): Path<(String, String)>,
) -> Json<mailrs_core_api::method::thread::FindThreadByMessageIdResponse> {
    let thread_id = state
        .mailbox
        .thread_for_message_id(&user, &message_id)
        .unwrap_or(None);
    Json(mailrs_core_api::method::thread::FindThreadByMessageIdResponse { thread_id })
}

/// `GET /v1/users/{user}/messages/by-uid/{uid}` — look up a message by
/// the user-scoped uid index (populated by `deliver_message` /
/// `mailrs-fastcore-backfill-uid-index`). Returns the JSON MessageWire.
pub(crate) async fn get_message_by_uid_for_user(
    State(state): State<Arc<FastcoreState>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    match state.mailbox.get_message_by_uid(&user, uid) {
        Ok(Some(bytes)) => Ok((
            [("content-type", "application/json")],
            String::from_utf8(bytes).unwrap_or_default(),
        )
            .into_response()),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, %user, %uid, "get_message_by_uid failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// `GET /v1/users/{user}/mailboxes` — returns the INBOX + standard IMAP
/// folders. Counts derived from kevy zsets so no spg touch.
/// This is a minimum-viable shape — future phase populates true
/// per-mailbox metadata via mailbox-kevy `list_mailboxes` when the
/// mailbox → messages sub-index lands.
pub(crate) async fn list_mailboxes(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<mailrs_core_api::method::mailbox::ListMailboxesResponse> {
    use mailrs_core_api::method::mailbox::{ListMailboxesResponse, MailboxWire};
    let total = state
        .mailbox
        .count_thread_ids_by_activity_via_table(&user)
        .unwrap_or(0) as u32;
    let unseen = state
        .mailbox
        .count_thread_ids_by_flag_via_table(&user, "unread")
        .unwrap_or(0) as u32;
    let items = vec![
        MailboxWire {
            id: 1,
            user: user.clone(),
            name: "INBOX".to_string(),
            uidvalidity: 1,
            uidnext: total + 1,
            highest_modseq: total as u64,
        },
        MailboxWire {
            id: 2,
            user: user.clone(),
            name: "Sent".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
        MailboxWire {
            id: 3,
            user: user.clone(),
            name: "Drafts".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
        MailboxWire {
            id: 4,
            user: user.clone(),
            name: "Junk".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
        MailboxWire {
            id: 5,
            user,
            name: "Trash".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
    ];
    let _ = unseen;
    Json(ListMailboxesResponse { items })
}

/// `POST /v1/users/{user}/conversations:by-thread-ids` — hydrate full
/// conversation rows for a set of thread_ids (search results),
/// preserving the requested order (G10).
pub(crate) async fn conversations_by_thread_ids(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ConversationsByIdsRequest>,
) -> Json<conv::ConversationsByIdsResponse> {
    // Each caller's own copy. Reading the shared hash here served one
    // owner's counters and flags to the other, and this is the endpoint
    // the client uses to refresh a conversation it already has open.
    let items = req
        .thread_ids
        .iter()
        .filter_map(|tid| {
            state
                .mailbox
                .get_thread_for_user(&user, tid)
                .ok()
                .flatten()
                .map(row_to_wire)
        })
        .collect();
    Json(conv::ConversationsByIdsResponse { items })
}

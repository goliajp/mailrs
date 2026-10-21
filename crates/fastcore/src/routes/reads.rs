//! The read side: conversation lists, search, counts, thread contents.
//!
//! Each of these reads **this user's** membership row, not the shared
//! thread hash. The shared one has no user segment, so on a thread two
//! accounts both received it holds whichever owner wrote last.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::*;
/// One reading of a conversation filter, shared by everything that has
/// to agree with the list.
///
/// "Mark all as read" marks whatever this list is showing, so it has
/// to ask the same question the list asked. Two copies of this
/// mapping is two chances to answer differently — and a mark-all that
/// marks a slightly different set than the rows on screen is a
/// feature that looks like it worked.
pub(crate) fn threads_filter(
    f: &mailrs_core_api::types::ConversationFilter,
) -> ListThreadsFilter<'_> {
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
    ListThreadsFilter {
        category: f.category.as_deref(),
        folder,
        pinned: false,
        archived: f.archived,
        has_unread: f.unread.unwrap_or(false),
        has_action: false,
        starred: f.starred.unwrap_or(false),
        before_ts: f.before_ts,
        accounts: f.accounts.clone(),
        // The caller's request, not a policy: whether a conversation
        // is held was decided at receive time. `Only` — the review
        // screen's reading — is reached through its own route rather
        // than through this filter, so a client cannot ask an
        // ordinary list for nothing but held mail by accident.
        quarantine: if f.include_quarantined {
            QuarantineScope::Include
        } else {
            QuarantineScope::Hide
        },
    }
}

/// `POST /v1/users/{user}/conversations:list`.
pub(crate) async fn list_conversations(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ListConversationsRequest>,
) -> Json<conv::ListConversationsResponse> {
    let f = &req.filter;
    let filter = threads_filter(f);
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
                .count_thread_ids_by_category_via_table(
                    &_user,
                    cat,
                    mailrs_mailbox_kevy::ArchiveScope::Live,
                )
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
            // deleting a failed send from the Send list flags its copy
            if w.flags & mailrs_core_api::method::message::FLAG_DELETED != 0 {
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
        .count_thread_ids_by_activity_via_table(&user, mailrs_mailbox_kevy::ArchiveScope::All)
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

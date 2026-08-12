//! The verbs the conversation list issues: read, star, pin, archive,
//! snooze, the triage moves, and the batch form of each.

use axum::extract::Query;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use mailrs_core_api::method::conversation as wire;

use crate::WebState;
use crate::handlers::conversations::*;

/// Batch mutation request/response — matches the UI's `useBatchMutation`.
#[derive(Debug, serde::Deserialize)]
pub struct BatchRequest {
    pub action: String,
    pub thread_ids: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct BatchResponse {
    pub failed: u32,
    pub message: Option<String>,
    pub processed: u32,
    pub success: bool,
    /// Which ones did not go through.
    ///
    /// The loop below has always known this and reported only a count,
    /// so a caller that removed fifty rows optimistically could learn
    /// that three failed and not which three — leaving it to roll back
    /// everything or nothing. iOS avoided the route entirely for that
    /// reason and sent fifty separate requests instead.
    ///
    /// Empty on success, and omitted from the JSON when empty so the
    /// shape older clients parse does not change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_thread_ids: Vec<String>,
}

/// POST /api/conversations/batch — apply the same mutation across many
/// threads. Fires each individually against fastcore (kevy mutations are
/// idempotent + fast, ~2 ms each). Runs sequentially; a partial failure
/// still lets the successes stick.
pub async fn batch_mutation(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<BatchRequest>,
) -> Result<Json<BatchResponse>, StatusCode> {
    let action = req.action.as_str();
    let mut processed = 0u32;
    let mut failed = 0u32;
    let mut failed_ids: Vec<String> = Vec::new();
    for tid in &req.thread_ids {
        let f = &state.core;
        let r = match action {
            "read" => f.mark_thread_read(&user, tid).await.map(|_| ()),
            "unread" => f.mark_thread_unread(&user, tid).await.map(|_| ()),
            "star" => f.star_thread(&user, tid).await.map(|_| ()),
            "unstar" => f.unstar_thread(&user, tid).await.map(|_| ()),
            "archive" => f.archive_thread(&user, tid).await.map(|_| ()),
            "unarchive" => f.unarchive_thread(&user, tid).await.map(|_| ()),
            "delete" => f.delete_thread(&user, tid).await.map(|_| ()),
            _ => Err(mailrs_core_api::error::CoreApiError::Internal(format!(
                "unknown batch action: {action}"
            ))),
        };
        match r {
            Ok(_) => processed += 1,
            Err(_) => {
                failed += 1;
                failed_ids.push(tid.clone());
            }
        }
    }
    Ok(Json(BatchResponse {
        failed,
        failed_thread_ids: failed_ids,
        message: None,
        processed,
        success: failed == 0,
    }))
}

/// POST /api/conversations/{thread_id}/read
pub async fn mark_thread_read(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .mark_thread_read(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/mark-all-read — sweep every unread thread
/// for the current user. The old "Mark all as read" button was only
/// batching the currently-loaded pagination slice; with 99+ unread
/// spread across pages the tail stayed untouched. This endpoint fixes
/// that by walking the has_unread zset server-side.
/// `POST /api/conversations/mark-all-read`
///
/// With no query parameters this is the whole mailbox, which is what
/// the web has always called and what the name says. With the *same*
/// query the conversation list takes — `folder`, `unread`, `starred`,
/// `archived` — it marks that list instead, because "mark all as
/// read" pressed inside Notifications should not silence the inbox.
///
/// Scoped server-side rather than by sending thread ids: a client can
/// only name the page it has loaded, and marking 50 of 1,458 would
/// look finished and do a fraction.
pub async fn mark_all_read(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<crate::handlers::conversations::ListQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let scoped = q.folder.is_some() || q.unread.is_some() || q.starred.is_some() || q.archived;
    let flipped = if scoped {
        let filter = mailrs_core_api::types::ConversationFilter {
            limit: 0,
            before_ts: None,
            category: q.category,
            domains: None,
            archived: q.archived,
            folder: q.folder,
            unread: q.unread,
            starred: q.starred,
            section: q.section,
        };
        state
            .core
            .mark_list_conversations_read(&user, &filter)
            .await
            .map_err(map_err)?
    } else {
        state
            .core
            .mark_all_conversations_read(&user)
            .await
            .map_err(map_err)?
    };
    Ok(Json(
        serde_json::json!({ "success": true, "flipped": flipped }),
    ))
}

/// POST /api/conversations/{thread_id}/star
pub async fn star_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .star_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/archive
pub async fn archive_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .archive_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/mark-junk
/// v2.4.1 Phase 3 (RFC-B §3.4) — move thread to Junk. Does NOT
/// modify the recipient's whitelist / blacklist — per plan §D4,
/// a single mark-junk is not a full sender block.
pub async fn mark_junk(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .mark_junk(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/mark-not-junk
/// v2.4.1 Phase 3 (RFC-B §3.4) — move thread to Inbox AND add
/// the thread's senders to the recipient's whitelist so future
/// arrivals from the same sender bypass the score threshold when
/// authed (§D5 requires SPF or DKIM pass at delivery time — see
/// `crates/inbound/src/decision.rs`).
pub async fn mark_not_junk(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    // 1) Extract the thread's senders_csv (via core RPC — thread data
    //    lives in fastcore's embedded kevy, NOT the network kevy that
    //    with_kevy connects to; the earlier version read a
    //    mailrs:thread:{tid} hash that never existed on the network
    //    side, so senders was always empty and the whitelist below
    //    never got written).
    //    SADD every distinct address into the recipient's whitelist,
    //    which is stored on the network kevy where the receiver's
    //    spam-list snapshot loader reads it (`crates/receiver/src/
    //    spam_lists.rs:41`, key `spam:{user}:whitelist`).
    //    Best-effort — kevy / RPC errors don't fail the mark-not-junk
    //    action; the folder move below is the load-bearing part.
    let senders_csv = match state
        .core
        .conversations_by_thread_ids(
            &user,
            &wire::ConversationsByIdsRequest {
                folder: None,
                thread_ids: vec![thread_id.clone()],
            },
        )
        .await
    {
        Ok(resp) => resp
            .items
            .into_iter()
            .next()
            .map(|c| c.participants)
            .unwrap_or_default(),
        Err(e) => {
            tracing::warn!(err = ?e, %user, %thread_id, "mark_not_junk: thread lookup failed; whitelist not updated");
            String::new()
        }
    };

    if !senders_csv.is_empty() {
        let user_lc = user.to_lowercase();
        let senders = senders_csv;
        let _ = crate::handlers::kevy_util::with_kevy(move |c| {
            let wl_key = format!("spam:{user_lc}:whitelist");
            // Bare addresses only: delivery compares the envelope
            // sender, so a stored `Name <addr>` never matches and the
            // whitelist silently does nothing.
            for addr in mailrs_core_api::types::sender_addresses(&senders) {
                if addr == user_lc {
                    // don't whitelist the owner's own address
                    continue;
                }
                let _ = c.sadd(wl_key.as_bytes(), &[addr.as_bytes()]);

                // Also exempt the sender from greylisting. Marking
                // not-junk says "I want this mail"; greylisting then
                // defers their next message until the sender retries,
                // which reads as the mail arriving late. Delivery and
                // receipt outrank protection, and the user has already
                // made the call for this address.
                let entry = serde_json::json!({
                    "id": 0,
                    "address_or_domain": addr,
                    "list_type": "white",
                    "created_at": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                });
                if let Ok(payload) = serde_json::to_vec(&entry) {
                    // Field id = the address itself, so marking the
                    // same sender twice updates one entry instead of
                    // accumulating duplicates.
                    let field = format!("not-junk:{addr}");
                    let _ = c.hset(
                        b"admin:greylist:local-lists",
                        &[(field.as_bytes(), payload.as_slice())],
                    );
                }
            }
            Ok(())
        });
    }

    // 2) Move the thread out of Junk into Inbox on the mailbox side.
    state
        .core
        .mark_not_junk(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/mark-notification
/// v2.9 triage — move thread into the Notifications bucket + train.
pub async fn mark_notification(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .mark_notification(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/mark-promotion
/// v2.9 triage — move thread into the Promotions bucket + train.
pub async fn mark_promotion(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .mark_promotion(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/move-to-inbox
/// v2.9 triage — move thread back into the Inbox bucket + train.
pub async fn move_to_inbox(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .move_to_inbox(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unread
pub async fn mark_thread_unread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .mark_thread_unread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unstar
pub async fn unstar_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .unstar_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/pin
pub async fn pin_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .pin_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unpin
pub async fn unpin_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .unpin_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unarchive
pub async fn unarchive_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .unarchive_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// DELETE /api/conversations/{thread_id}
pub async fn delete_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .delete_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

#[derive(Debug, serde::Deserialize)]
pub struct SnoozeBody {
    pub snoozed_until: i64,
}

/// PUT /api/conversations/{thread_id}/snooze
pub async fn snooze_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    Json(req): Json<SnoozeBody>,
) -> Result<StatusCode, StatusCode> {
    let wire_req = mailrs_core_api::method::thread::SnoozeRequest {
        snoozed_until: req.snoozed_until,
    };
    state
        .core
        .snooze_thread(&user, &thread_id, &wire_req)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// DELETE /api/conversations/{thread_id}/snooze
pub async fn unsnooze_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .unsnooze_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

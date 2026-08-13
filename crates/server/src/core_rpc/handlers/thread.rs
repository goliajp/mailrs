//! Handlers for `mailrs_core_api::method::thread`.
//!
//! 12 mutate + a few read endpoints. Each thin pass-through.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::thread as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/threads/{thread_id}/messages
pub async fn list_thread_messages(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ListThreadMessagesResponse>, StatusCode> {
    let rows = state
        .mailbox
        .list_thread_messages(&user, &thread_id, None)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "list_thread_messages failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let user_clone = user.clone();
    let items = rows
        .iter()
        .map(|m| {
            let mut w: mailrs_core_api::method::message::MessageWire = m.into();
            w.user_address = user_clone.clone();
            w
        })
        .collect();
    Ok(Json(wire::ListThreadMessagesResponse { items }))
}

/// Helper that wraps `Result<u32, sqlx::Error>` into a `ThreadActionResponse`.
async fn into_action_response(
    res: Result<u32, sqlx::Error>,
    context: &'static str,
    user: &str,
    thread_id: &str,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let affected = res.map_err(|e| {
        tracing::warn!(error = %e, user, thread_id, context, "thread mutate failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::ThreadActionResponse {
        affected,
        new_modseq: 0,
    }))
}

/// POST /v1/users/{user}/threads/{thread_id}/read
pub async fn mark_read(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state
        .mailbox
        .mark_thread_read(&user, &thread_id, None)
        .await;
    into_action_response(res, "mark_read", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/unread
pub async fn mark_unread(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.mark_thread_unread(&user, &thread_id).await;
    into_action_response(res, "mark_unread", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/star
pub async fn star(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.star_thread(&user, &thread_id).await;
    into_action_response(res, "star", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/unstar
pub async fn unstar(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.unstar_thread(&user, &thread_id).await;
    into_action_response(res, "unstar", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/pin
pub async fn pin(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.pin_thread(&user, &thread_id).await;
    into_action_response(res, "pin", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/unpin
pub async fn unpin(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.unpin_thread(&user, &thread_id).await;
    into_action_response(res, "unpin", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/archive
pub async fn archive(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.archive_thread(&user, &thread_id).await;
    into_action_response(res, "archive", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/unarchive
pub async fn unarchive(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.unarchive_thread(&user, &thread_id).await;
    into_action_response(res, "unarchive", &user, &thread_id).await
}

/// PUT /v1/users/{user}/threads/{thread_id}/snooze
pub async fn snooze(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<wire::SnoozeRequest>,
) -> Result<StatusCode, StatusCode> {
    use chrono::TimeZone;
    let until = chrono::Utc
        .timestamp_opt(req.snoozed_until, 0)
        .single()
        .ok_or(StatusCode::BAD_REQUEST)?;
    state
        .mailbox
        .snooze_thread(&user, &thread_id, until)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "snooze failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/users/{user}/threads/{thread_id}/snooze
pub async fn unsnooze(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .unsnooze_thread(&user, &thread_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "unsnooze failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/users/{user}/threads/{thread_id}
pub async fn delete_thread(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .delete_thread(&user, &thread_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "delete_thread failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/users/{user}/threads/{thread_id}/messages — ingest a message
/// into the PG store (the landing route for `mailrs-core-sync` running
/// kevy→PG; the fastcore equivalent lives in fastcore's `deliver_message`).
///
/// Message-ID idempotent: re-delivering the same message is a no-op that
/// echoes the existing thread, so a re-run of sync never double-inserts.
/// The message's raw bytes are NOT transported — `blob_ref` points at the
/// shared maildir both cores mount, so only metadata/threading lands here.
pub async fn deliver_message(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<wire::DeliverMessageRequest>,
) -> Result<Json<wire::DeliverMessageResponse>, StatusCode> {
    use mailrs_core_api::method::message::MessageWire;
    use mailrs_mailbox::{InsertMessage, MailboxStore};

    // idempotency: already ingested → echo, do not re-insert
    if let Ok(Some(existing)) = state
        .mailbox
        .find_by_message_id(&user, &req.message_id)
        .await
    {
        return Ok(Json(wire::DeliverMessageResponse {
            thread_id: existing.thread_id,
            message_id: req.message_id,
        }));
    }

    let wire: MessageWire = serde_json::from_str(&req.payload_wire_json).map_err(|e| {
        tracing::warn!(error = %e, user = %user, "deliver_message: bad payload_wire_json");
        StatusCode::BAD_REQUEST
    })?;

    // ensure the destination mailbox exists (sync lands everything in INBOX;
    // per-mailbox placement is not preserved cross-backend by design).
    // Direct idempotent INSERT rather than get_mailbox-then-create — the
    // DomainStore read path can serve a stale cached miss, after which
    // create_mailbox is skipped and index_message's `UPDATE mailboxes …
    // RETURNING` finds no row ("no rows returned"). ON CONFLICT makes this
    // safe + cache-independent.
    const MAILBOX: &str = "INBOX";
    sqlx::query(
        "INSERT INTO mailboxes (user_address, name, uidvalidity) VALUES ($1, $2, 1) \
         ON CONFLICT (user_address, name) DO NOTHING",
    )
    .bind(&user)
    .bind(MAILBOX)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, user = %user, "deliver_message: ensure INBOX failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // `req.unread` wins over the payload's `\Seen` bit.
    //
    // The request carries the read state twice and the two copies do not agree
    // for most of a mailbox: `core-sync` computes `unread` as "the user is not
    // among the senders", while `payload_wire_json` carries the IMAP flags,
    // where bit 1 is `\Seen`. So every already-read message somebody else sent
    // arrives with `unread: true` and `\Seen` set.
    //
    // kevy stores the summary field; this side counted `flags & 1`, so the two
    // cores reported opposite unread counts for the same mail — found by
    // `core_rpc/tests/two_lane.rs`, which is what a switch would have shipped:
    // an inbox whose badge and bold rows all invert.
    //
    // The summary field is the authority because it is the one the sender-based
    // rule produced deliberately, and because the flag in a migrated payload is
    // the *source* store's flag, which this store's own uid space no longer
    // refers to.
    const SEEN: u32 = 1;
    let flags = if req.unread {
        wire.flags & !SEEN
    } else {
        wire.flags | SEEN
    };

    let input = InsertMessage {
        user: &user,
        mailbox_name: MAILBOX,
        blob_ref: wire.blob_ref.as_str(),
        sender: &wire.sender,
        recipients: &wire.recipients,
        subject: &wire.subject,
        size: wire.size,
        date: wire.date,
        internal_date: wire.internal_date,
        message_id: &req.message_id,
        in_reply_to: &wire.in_reply_to,
        thread_id: &thread_id,
        flags,
    };
    let inserted = state.mailbox.insert_message(input).await.map_err(|e| {
        tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "deliver_message: insert failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Keep `req.category`, which this lane otherwise discards.
    //
    // `list_conversations` reads a thread's category from `email_analysis`,
    // defaulting to `'general'` when no row is there — and nothing on an ingest
    // path ever wrote one. So the field arrived, was dropped, and every
    // migrated thread came back `general`: a switch to this core collapsed
    // inbox / promotions / notifications / spam into one tab, silently, because
    // `general` is a plausible answer rather than an error.
    //
    // Only the category column is written. The rest of the row is genuinely
    // absent — no summary, no risk score, no embedding — and a placeholder
    // would claim this message had been analysed.
    if !req.category.is_empty()
        && let Err(e) = sqlx::query(
            "INSERT INTO email_analysis (message_id, category) VALUES ($1, $2) \
             ON CONFLICT (message_id) DO UPDATE SET category = EXCLUDED.category",
        )
        .bind(inserted.id)
        .bind(&req.category)
        .execute(&state.pool)
        .await
    {
        // Not fatal: the message is delivered and readable. Losing the
        // category puts the thread in the default tab, which is worth a
        // line in the log and not worth rejecting the mail for.
        tracing::warn!(
            error = %e, user = %user, thread_id = %thread_id,
            category = %req.category,
            "deliver_message: category not recorded"
        );
    }

    Ok(Json(wire::DeliverMessageResponse {
        thread_id,
        message_id: req.message_id,
    }))
}

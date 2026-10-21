//! Delivering a message into this store through the core-api.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::thread as wire;

use crate::core_rpc::CoreRpcState;

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
    if let Some(existing) = state
        .mailbox
        .find_by_message_id(&user, &req.message_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, "deliver_message: lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
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

    // The user's mailboxes exist before anything lands in one, as on the
    // receiving path. Mail the user sent goes to Sent: that is where this
    // store's views find it, for the Sent folder, the sent count and the
    // exclusion of sent-only threads from Inbox.
    state
        .mailbox
        .ensure_default_mailboxes(&user)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, "deliver_message: mailboxes not created");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mailbox = if mailrs_rfc5322::list_contains(&req.senders_csv, &user) {
        "Sent"
    } else {
        "INBOX"
    };

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
        mailbox_name: mailbox,
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
    // The preview line and the category the request carries. A message
    // delivered with no category is inbox mail, as the kevy core files it;
    // the views read a thread's category from its newest classified message,
    // so every arrival writes one.
    let category = match req.category.trim() {
        "" => "inbox",
        c => c,
    };
    let facts = mailrs_mailbox::pg::ArrivalFacts {
        snippet: &req.latest_preview,
        category,
        sender_trust: &wire.sender_trust,
    };
    state
        .mailbox
        .record_arrival(input, facts)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "deliver_message: insert failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(wire::DeliverMessageResponse {
        thread_id,
        message_id: req.message_id,
    }))
}

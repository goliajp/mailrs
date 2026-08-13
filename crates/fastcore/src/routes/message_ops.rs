//! Writing a message into a user's mailbox, and the per-message verbs.
//!
//! `deliver_message` is the one both the web send path and the sender
//! mirror go through, so a copy the user sent lands the same way a copy
//! they received does.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::*;

/// `GET /v1/users/{user}/sent-messages` — one row per outbound message
/// (not per thread). Walks the user's sent-thread index, reads each
/// thread's messages, keeps only the ones this user actually sent, and
/// returns them newest-first with the recipient (To). Reuses the existing
/// per-thread message store — no dedicated sent-message index.
/// Every thread the user has written in, newest first.
///
/// The declared `is_sender` axis is the authority: it is maintained at
/// ingest through the membership row, and its own declaration says so —
/// "The Sent axis has the same shape: key on the flag, filter to the user,
/// sort by recency."
///
/// `user_threads_sent` is gone. That zset was legacy — it is in
/// `all_user_thread_zsets`, the list `drop-legacy-zsets` deletes — and
/// reading it was why a delivered reply was missing from Send on
/// 2026-07-30: nothing on the ingest path writes it, and its only refiller
/// was the periodic maildir sweep, which backs off exponentially while
/// idle.
///
/// It was unioned in for one release while the two sets were compared.
/// `maintenance:sent-axis-shadow` across all 13 accounts on 2026-07-31
/// reported `only_in_zset_live: 0` — the only divergence was three thread
/// ids the zset still named after a merge had emptied them, which hold no
/// messages and therefore contribute nothing to this list.
///
/// Paged rather than capped. A silent limit here would drop the oldest
/// sent threads out of the list with nothing to say it had happened.
pub(crate) fn sent_thread_ids(state: &Arc<FastcoreState>, user: &str) -> Vec<String> {
    const PAGE: usize = 1000;
    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut offset = 0usize;
    loop {
        let page = match state.mailbox.list_thread_ids_by_flag_via_table(
            user,
            "is_sender",
            PAGE,
            offset,
            None,
        ) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(err = %e, %user, offset, "sent axis page failed");
                break;
            }
        };
        let short = page.len() < PAGE;
        for tid in page {
            if seen.insert(tid.clone()) {
                out.push(tid);
            }
        }
        if short {
            break;
        }
        offset += PAGE;
    }

    out
}

/// Remove the maildir file at `blob_ref` — the disk counterpart of
/// `KevyMailboxStore::delete_thread`. Tries both `cur/` and `new/`
/// because a message hops between them as its `\Seen` flag flips.
///
/// Best-effort: an fs error (permission, race, already gone) logs a
/// warning but must not fail the surrounding delete — the point of
/// this helper is to prevent self-heal from resurrecting the row on
/// its next tick, and a missing file already satisfies that. Returns
/// true if any file was actually unlinked (helpful for the caller's
/// log line).
pub(crate) fn unlink_maildir_file(user: &str, blob_ref: &str) -> bool {
    if blob_ref.is_empty() {
        return false;
    }
    let Some((local, domain)) = user.split_once('@') else {
        return false;
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = std::path::PathBuf::from(root).join(domain).join(local);
    let (sub, name) = match blob_ref.split_once('/') {
        Some((s, n)) => (Some(s), n),
        None => (None, blob_ref),
    };
    let mut removed = false;
    for leaf in ["cur", "new"] {
        let path = match sub {
            Some(s) => base.join(s).join(leaf).join(name),
            None => base.join(leaf).join(name),
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {
                removed = true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(
                    error = %e, path = %path.display(),
                    "delete_thread: could not unlink maildir file"
                );
            }
        }
    }
    removed
}

/// `POST /v1/admin/threads:split-message` `{user, message_id}` — move a
/// message out of its thread into its own conversation (manual fix for
/// topic-change replies that were glued before the subject gate landed).
pub(crate) async fn split_message_route(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<serde_json::Value>,
) -> axum::response::Response {
    let user = req["user"].as_str().unwrap_or("");
    let mid = req["message_id"].as_str().unwrap_or("");
    if user.is_empty() || mid.is_empty() {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    match state.mailbox.split_message_to_new_thread(user, mid) {
        Ok(Some(tid)) => Json(serde_json::json!({"thread_id": tid})).into_response(),
        Ok(None) => axum::http::StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(err = %e, %user, %mid, "split_message failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Uniform mutation response — matches monolith's `ThreadActionResponse`
/// JSON shape so the core-rpc client's deserializer succeeds. Fastcore's
/// mutations are idempotent (mark_seen / set_pinned / set_starred / ...
/// are all noop-safe when the target thread is already in the requested
/// state or missing). Return 200 unconditionally so the UI's optimistic
/// patch never rolls back — a missing thread row simply means "nothing
/// to do" and the list refetch will reconcile.
pub(crate) fn action_result(_found: bool) -> axum::response::Response {
    use axum::response::IntoResponse;
    Json(th::ThreadActionResponse {
        affected: 1,
        new_modseq: 0,
    })
    .into_response()
}

/// POST /v1/users/{user}/threads/{thread_id}/messages — the sent /
/// draft / import write path. Mirrors what the inbound ingest loop
/// does, but the caller controls the metadata (senders_csv, unread,
/// category) so it can synthesize a "user is the sender" arrival.
///
/// Executes 3 atomic-ish steps:
///   1. `record_message_arrival` — thread aggregate + activity/category
///      zsets + has_unread toggle if `unread=true`
///   2. `upsert_message` — write `mailrs:msg:<mid>` blob (verbatim
///      `payload_wire_json`) + zadd `mailrs:thread:<tid>:messages`
///   3. `upsert_thread` — re-read the aggregate we just updated and
///      re-emit every index, most importantly `user_threads_sent` (adds
///      when `senders_csv_contains_user`) and `has_unread`
pub(crate) async fn deliver_message(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<th::DeliverMessageRequest>,
) -> axum::response::Response {
    use mailrs_mailbox_kevy::MessageArrival;
    let arrival = MessageArrival {
        thread_id: &thread_id,
        user: &user,
        subject: &req.subject,
        senders_csv: &req.senders_csv,
        latest_date: req.latest_date,
        latest_preview: &req.latest_preview,
        category: &req.category,
        unread: req.unread,
        is_own: mailrs_mailbox_kevy::senders_csv_contains_user(&req.senders_csv, &user),
    };

    if let Err(e) = state.mailbox.record_message_arrival(&arrival) {
        tracing::error!(err = %e, %user, %thread_id, "record_message_arrival failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Side sink so contacts autocomplete stays live on webapi-
    // driven deliveries (mirror-send, forward-into-thread, etc.).
    let _ = state.notify.send(user.clone());
    crate::live_sync::publish_new_mail(
        &user,
        &thread_id,
        &req.senders_csv,
        &req.subject,
        &req.latest_preview,
    );
    crate::live_sync::upsert_contacts(&user, &req.senders_csv);

    // Allocate the per-user persistent uid HERE, not at the caller —
    // fastcore owns the uid space. mirror_send used to pass wires with
    // uid=0 straight through, so every web-sent message produced
    // /api/mail/messages/0/attachments/... URLs that 404'd (attachment
    // preview / raw / flags all resolve via the uid index).
    // allocate_uid is idempotent per (user, message_id).
    let payload = match state.mailbox.allocate_uid(&user, &req.message_id) {
        Ok(uid) if uid != 0 => {
            let _ = state.mailbox.index_uid(&user, uid, &req.message_id);
            match serde_json::from_str::<mailrs_core_api::method::message::MessageWire>(
                &req.payload_wire_json,
            ) {
                Ok(mut wire) => {
                    wire.uid = uid;
                    serde_json::to_string(&wire).unwrap_or_else(|_| req.payload_wire_json.clone())
                }
                Err(_) => req.payload_wire_json.clone(),
            }
        }
        _ => req.payload_wire_json.clone(),
    };
    // The sent copy is this user's own: its maildir file is in their
    // mailbox and its uid is theirs. Parsed back out of the payload so the
    // per-user row records what was actually written.
    let sent_wire: Option<mailrs_core_api::method::message::MessageWire> =
        serde_json::from_str(&payload).ok();
    let sent_facts = sent_wire
        .as_ref()
        .map(|w| mailrs_mailbox_kevy::UserMessageFacts {
            blob_ref: &w.blob_ref,
            uid: w.uid,
            flags: w.flags,
            modseq: w.modseq,
        });
    let fallback = mailrs_mailbox_kevy::UserMessageFacts {
        blob_ref: "",
        uid: 0,
        flags: 0,
        modseq: 0,
    };
    if let Err(e) = state.mailbox.upsert_user_message(
        user.as_str(),
        &thread_id,
        &req.message_id,
        req.latest_date,
        payload.as_bytes(),
        sent_facts.as_ref().unwrap_or(&fallback),
    ) {
        tracing::error!(err = %e, %user, %thread_id, "upsert_user_message failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    // register the (sent-copy) message id → thread so a remote reply
    // citing it via In-Reply-To resolves into this conversation instead
    // of opening a fragment (the v2.9.5 threading fix's key edge).
    let _ = state
        .mailbox
        .set_thread_for_message_id(&user, &req.message_id, &thread_id);

    // Re-emit thread row so index zsets (sent, has_unread, etc.) reflect
    // the new senders_csv / unread_count state. We read the row we just
    // wrote and hand it to upsert_thread which owns the index fanout.
    match state.mailbox.get_thread(&thread_id) {
        Ok(Some(row)) => {
            if let Err(e) = state.mailbox.upsert_thread(&user, &row) {
                tracing::warn!(err = %e, %user, %thread_id, "upsert_thread reindex failed");
            }
        }
        Ok(None) => {
            tracing::warn!(%user, %thread_id, "get_thread returned None right after write");
        }
        Err(e) => {
            tracing::warn!(err = %e, %user, %thread_id, "get_thread failed");
        }
    }

    if req.uid > 0
        && let Err(e) = state.mailbox.index_uid(&user, req.uid, &req.message_id)
    {
        tracing::warn!(err = %e, %user, uid = req.uid, "index_uid failed");
    }

    Json(th::DeliverMessageResponse {
        thread_id,
        message_id: req.message_id,
    })
    .into_response()
}

/// Queue a delivery for every subscription that admits this message.
///
/// Best-effort by design: a webhook that cannot be queued must not fail the
/// delivery of the mail itself. Failures are logged rather than swallowed,
/// which is the difference between this and the class of silence the
/// 2026-07-30 audit was about.
pub(crate) fn enqueue_webhooks_for_arrival(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
    sender: &str,
    subject: &str,
) {
    use mailrs_core_sidestate::families::{webhook_outbox, webhooks};

    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let subs = match webhooks::matching(&mut conn, user, sender, thread_id) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(err = %e, %user, "webhook: could not read subscriptions");
            return;
        }
    };
    if subs.is_empty() {
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let timestamp = chrono::DateTime::from_timestamp(now, 0)
        .map(|t| t.to_rfc3339())
        .unwrap_or_default();
    let payload =
        webhooks::new_message_payload(user, thread_id, sender, subject, "", &timestamp).to_string();
    for sub in subs {
        match webhook_outbox::enqueue(&mut conn, sub.id, user, &payload, now) {
            Ok(id) => tracing::info!(entry = id, subscription = sub.id, "webhook queued"),
            Err(e) => tracing::warn!(err = %e, subscription = sub.id, "webhook: enqueue failed"),
        }
    }
}

/// `POST /v1/users/{user}/messages/{uid}/flags` — patch the flags
/// bitmask on a message blob. Also reconciles the thread's `has_unread`
/// zset via `mark_seen` / `mark_unread` when `\Seen` toggled.
pub(crate) async fn set_message_flags_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, uid)): Path<(String, u32)>,
    Json(req): Json<adm::SetMessageFlagsRequest>,
) -> axum::response::Response {
    let bytes = match state.mailbox.get_message_by_uid(&user, uid) {
        Ok(Some(b)) => b,
        _ => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    let mut wire: mailrs_core_api::method::message::MessageWire =
        match serde_json::from_slice(&bytes) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(err = %e, %user, %uid, "wire parse failed");
                return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
    let old_flags = wire.flags;
    let new_flags = req.flags;
    wire.flags = new_flags;
    let json = match serde_json::to_vec(&wire) {
        Ok(v) => v,
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Err(e) = state.mailbox.upsert_user_message(
        user.as_str(),
        &wire.thread_id,
        &wire.message_id,
        wire.date,
        &json,
        &mailrs_mailbox_kevy::UserMessageFacts {
            blob_ref: &wire.blob_ref,
            uid: wire.uid,
            flags: wire.flags,
            modseq: wire.modseq,
        },
    ) {
        tracing::error!(err = %e, %user, %uid, "upsert_message failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    // Imported, not spelled out. This read `0b0000_0001` until the constant
    // became reachable from this lane — and a bitmask restated at a call
    // site is how one bit ends up meaning two things.
    use mailrs_core_api::method::message::FLAG_SEEN;
    let was_seen = (old_flags & FLAG_SEEN) != 0;
    let is_seen = (new_flags & FLAG_SEEN) != 0;
    if was_seen != is_seen && !wire.thread_id.is_empty() {
        let _ = if is_seen {
            state.mailbox.mark_seen(&user, &wire.thread_id)
        } else {
            state.mailbox.mark_unread(&user, &wire.thread_id)
        };
        // Reading over IMAP is still reading. Without this, engagement
        // would only ever be recorded for the web UI, and a user on
        // Apple Mail or Thunderbird would look like they never open
        // anything — a systematic hole in the data the ranker learns
        // from, invisible until the learner started producing nonsense.
        //
        // `was_seen != is_seen` already makes this a genuine unread ->
        // read transition, so re-syncing an unchanged flag records
        // nothing.
        if is_seen {
            let event = crate::importance::read_event(&state, &wire.thread_id, now_secs());
            crate::importance::record_engagement(&state, &user, &wire.thread_id, event);
        }
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

pub(crate) fn row_to_wire(r: ThreadRow) -> ConversationSummaryWire {
    ConversationSummaryWire {
        thread_id: r.thread_id,
        subject: r.subject,
        participants: r.senders_csv,
        message_count: r.count.max(0) as u32,
        unread_count: r.unread_count.max(0) as u32,
        last_date: r.latest_date,
        category: r.category,
        flagged: r.starred,
        snippet: r.latest_preview,
        pinned: r.pinned,
        archived: r.archived,
        importance_level: r.importance_level,
        importance_score: r.importance_score as f32,
        requires_action: r.requires_action,
        sent_count: r.sent_count.max(0) as u32,
        // The reader's own, off their membership row — this is the
        // row a client draws, and a zero here would mean no client
        // could ever say a thread is asleep.
        snoozed_until: r.snoozed_until,
    }
}

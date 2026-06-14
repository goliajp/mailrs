//! The core's post-delivery work: everything that happens to a message
//! after it is written to maildir — indexing, threading, the `NewMessage`
//! event, iTIP invite projection, and the spawned content / importance /
//! BIMI pass.
//!
//! Extracted verbatim from the SMTP DATA handler (S1.3) so it can later be
//! driven by a notification consumer instead of inline, and so the
//! receiver/core split (P5/P6) has a clean seam: the receiver writes the
//! maildir file, the core runs `process_delivered`. Behaviour is identical
//! to the old inline block — only the data it reads from the enclosing
//! scope is now carried by [`DeliveredMessage`] + [`ProcessDeps`].

use std::sync::Arc;

use hickory_resolver::TokioResolver;

use mailrs_mailbox::PgMailboxStore;

use super::headers::{extract_snippet, extract_subject_and_from};
use super::post_delivery::post_delivery_process;
use crate::event_bus::{EventBus, SmtpEvent};
use crate::pg::BackendPool;

/// One message successfully written to maildir, ready for indexing and the
/// post-delivery pass. Built by the DATA handler from the delivery result.
/// Derived values (subject, sender, thread_id, effective message-id) are
/// computed inside [`process_delivered`], not carried here.
pub(crate) struct DeliveredMessage {
    pub maildir_id: String,
    pub user: String,
    pub rcpt: String,
    pub rcpt_folder: String,
    pub reverse_path: String,
    pub full_message: Arc<Vec<u8>>,
    pub msg_message_id: String,
    pub msg_in_reply_to: String,
    pub msg_size: usize,
}

/// The core-side handles [`process_delivered`] needs, cloned once from the
/// `ConnectionContext` per DATA batch. Decoupled from the full context so
/// the receiver/core split doesn't have to thread `ConnectionContext`
/// across the boundary.
pub(crate) struct ProcessDeps {
    pub mailbox_store: Arc<PgMailboxStore>,
    pub event_bus: EventBus,
    pub outbound_queue: Option<BackendPool>,
    pub resolver: Option<Arc<TokioResolver>>,
    pub maildir_root: String,
}

/// Index a delivered message and run every post-delivery side effect:
/// ensure mailboxes → extract subject/from → effective message-id →
/// thread resolution → sieve folder → index → `NewMessage` event → iTIP
/// invite projection → spawn the content / importance / BIMI pass.
pub(crate) async fn process_delivered(msg: DeliveredMessage, deps: &ProcessDeps) {
    let mb_store = &deps.mailbox_store;
    let maildir_id_str = msg.maildir_id.clone();
    let user = msg.user.clone();
    let rcpt = msg.rcpt.as_str();
    let rcpt_folder = msg.rcpt_folder.clone();
    let reverse_path = msg.reverse_path.clone();
    let full_message = msg.full_message.clone();
    let msg_message_id = msg.msg_message_id.clone();
    let msg_in_reply_to = msg.msg_in_reply_to.clone();
    let msg_size = msg.msg_size;

    let _ = mb_store.ensure_default_mailboxes(&user).await;
    let now = chrono::Utc::now().timestamp();

    // single-pass extract: mail-parser builds a full Message tree per
    // call; calling extract_header twice parses twice. The _and_from
    // helper hands back both fields for one parse.
    let extract_started = std::time::Instant::now();
    let (subject, from_header) = extract_subject_and_from(&full_message);
    tracing::debug!(
        phase = "extract_subject_from",
        duration_us = extract_started.elapsed().as_micros() as u64,
        "stage complete"
    );
    // use From header for sender (includes display name); fall back to
    // SMTP envelope reverse_path
    let sender = if from_header.is_empty() {
        reverse_path.clone()
    } else {
        from_header
    };

    // generate a synthetic message-id if missing
    let effective_message_id = if msg_message_id.is_empty() {
        format!("{}.{}@mailrs.local", now, maildir_id_str)
    } else {
        msg_message_id.clone()
    };

    // resolve thread_id
    let thread_id = {
        let parent_tid = if !msg_in_reply_to.is_empty() {
            mb_store
                .find_thread_id_by_message_id(&user, &msg_in_reply_to)
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        mailrs_mailbox::threading::resolve_thread_id(
            &effective_message_id,
            &msg_in_reply_to,
            |_| parent_tid.clone(),
        )
    };

    // ensure sieve target folder exists
    if rcpt_folder != "INBOX" && rcpt_folder != "Junk" {
        let _ = mb_store.create_mailbox(&user, &rcpt_folder).await;
    }

    let index_started = std::time::Instant::now();
    let indexed_uid = mb_store
        .index_message(
            &user,
            &rcpt_folder,
            &maildir_id_str,
            &sender,
            rcpt,
            &subject,
            msg_size as u32,
            now,
            &effective_message_id,
            &msg_in_reply_to,
            &thread_id,
        )
        .await
        .ok();
    tracing::debug!(
        phase = "mailbox_index_message",
        duration_us = index_started.elapsed().as_micros() as u64,
        "stage complete"
    );

    // emit NewMessage event
    let snippet = extract_snippet(&full_message);
    deps.event_bus.emit(SmtpEvent::NewMessage {
        user: user.clone(),
        thread_id: thread_id.clone(),
        sender: sender.clone(),
        subject: subject.clone(),
        snippet,
    });

    // MRS-4: detect iTIP / iMIP invite parts and project the parsed
    // payload onto messages.invite_payload so the web client / macapp can
    // render an invite card without re-parsing.
    if let Some(uid) = indexed_uid
        && let Some(extracted) = crate::calendar::invite_extract::extract_invite_part(&full_message)
    {
        if let Ok(parsed) = mailrs_ical::parse_invite(&extracted.ics_bytes) {
            if let Ok(payload_json) = serde_json::to_value(&parsed) {
                let method_str = format!("{:?}", parsed.method).to_uppercase();
                match mb_store
                    .update_invite_payload(&user, &rcpt_folder, uid, &payload_json, &method_str)
                    .await
                {
                    Ok(Some(msg_id)) => {
                        deps.event_bus.emit(SmtpEvent::InviteReceived {
                            user: user.clone(),
                            message_id: msg_id,
                            method: method_str.clone(),
                            uid: parsed.uid.clone(),
                        });
                        // MRS-7: reconcile against the user's own calendar
                        // (only touches events the user has already RSVP'd).
                        if let Some(ref pool) = deps.outbound_queue {
                            let raw_ics = std::str::from_utf8(&extracted.ics_bytes).unwrap_or("");
                            match crate::calendar::reconcile::reconcile_inbound_invite(
                                pool, &user, &parsed, raw_ics,
                            )
                            .await
                            {
                                Ok(outcome) => {
                                    tracing::info!(
                                        user = %user,
                                        uid = %parsed.uid,
                                        method = %method_str,
                                        outcome = ?outcome,
                                        "reconcile",
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "reconcile failed for {user}/{}: {e}",
                                        parsed.uid,
                                    );
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::debug!(
                            "invite detected but message row not found for {user}/{rcpt_folder}/{uid}"
                        );
                    }
                    Err(e) => {
                        tracing::warn!("update_invite_payload failed: {e}");
                    }
                }
            }
        } else {
            tracing::debug!(
                "text/calendar part found but ical parse failed for {user}/{rcpt_folder}/{uid}"
            );
        }
    }

    // async post-delivery: contact upsert + content extraction + importance scoring
    let mb_store_bg = Arc::clone(mb_store);
    let user_bg = user.clone();
    let sender_bg = sender.clone();
    let maildir_id_bg = maildir_id_str.clone();
    let maildir_root_bg = deps.maildir_root.clone();
    let raw_headers =
        String::from_utf8_lossy(&full_message[..full_message.len().min(4096)]).to_string();
    // Arc-clone the function-level body (cheap refcount bump) instead of
    // copying its bytes.
    let full_msg_bg = Arc::clone(&full_message);
    let resolver_bg = deps.resolver.clone();
    tokio::spawn(async move {
        post_delivery_process(
            &mb_store_bg,
            &user_bg,
            &sender_bg,
            &maildir_id_bg,
            &maildir_root_bg,
            &raw_headers,
            &full_msg_bg,
            resolver_bg.as_deref(),
        )
        .await;
    });
}

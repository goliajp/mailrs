//! Shared helpers used across multiple `mail/` sub-modules.
//!
//! Anything in this file is reachable from sibling sub-modules via
//! `use super::common::*`. Items that need to remain callable from outside
//! the `mail::` module (e.g. `mcp/mod.rs`, `web/rsvp.rs`, `web/auth.rs`,
//! `web/jmap.rs`) are `pub(crate)` so the `pub(crate) use common::*` re-export
//! in `mod.rs` lifts them to the `mail::` path.

use std::sync::Arc;

use axum::Json;

use super::{SendResult, WebState};

mod build;

pub(crate) use build::*;

/// Attachment payload used by the send pipeline (multipart upload + forwarded
/// attachments + MCP send-with-attachments). Lives in `common.rs` because it
/// is shared by `send.rs` (handlers) and `crate::mcp::mod` (MCP tools).
pub(crate) struct AttachmentData {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// check if a sender address is allowed for the authenticated user
/// returns Ok(()) if allowed, Err(message) if not
pub(crate) fn verify_sender(
    from: &str,
    user: &str,
    permissions: &crate::permission::EffectivePermissions,
) -> Result<(), &'static str> {
    if from == user {
        return Ok(());
    }
    // check if from is an alias address owned by this user
    if permissions
        .send_as()
        .iter()
        .any(|a| a.eq_ignore_ascii_case(from))
    {
        return Ok(());
    }
    // super user or user with accessible domains
    let accessible = permissions.accessible_domains();
    if !accessible.is_empty()
        && let Some(domain) = from.rsplit_once('@').map(|(_, d)| d)
        && (permissions.is_super() || accessible.iter().any(|sd| sd.eq_ignore_ascii_case(domain)))
    {
        return Ok(());
    }
    Err("sender must match authenticated user")
}

/// resolve reply_to_thread_id into in_reply_to message-id and references
/// returns (resolved_in_reply_to, references)
pub(crate) async fn resolve_thread_reply(
    reply_to_thread_id: Option<&str>,
    in_reply_to: Option<&str>,
    user: &str,
    mb_store: Option<&mailrs_mailbox::PgMailboxStore>,
) -> (Option<String>, Vec<String>) {
    // explicit in_reply_to takes precedence
    if let Some(reply_to) = in_reply_to
        && !reply_to.is_empty()
    {
        let refs = match mb_store {
            Some(store) => store
                .get_thread_references(user, reply_to)
                .await
                .unwrap_or_default(),
            None => vec![],
        };
        return (Some(reply_to.to_string()), refs);
    }

    // resolve thread_id to last message's message-id
    if let (Some(thread_id), Some(store)) = (reply_to_thread_id, mb_store)
        && !thread_id.is_empty()
        && let Ok(Some(last_msg_id)) = store.get_last_message_id_in_thread(user, thread_id).await
    {
        let refs = store
            .get_thread_message_ids(user, thread_id)
            .await
            .unwrap_or_default();
        return (Some(last_msg_id), refs);
    }

    (None, vec![])
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn deliver_message(
    state: &Arc<WebState>,
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    raw: &[u8],
    message_id: &str,
    ts: i64,
) -> Json<SendResult> {
    deliver_message_ex(state, from, to, cc, bcc, raw, message_id, ts, None).await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn deliver_message_ex(
    state: &Arc<WebState>,
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    raw: &[u8],
    message_id: &str,
    ts: i64,
    scheduled_at: Option<i64>,
) -> Json<SendResult> {
    let all_recipients: Vec<String> = to
        .iter()
        .chain(cc.iter())
        .chain(bcc.iter())
        .map(|s| extract_address(s))
        .collect();

    let local_domains: Vec<String> = if let Some(ref ds) = state.domain_store {
        ds.list_domains()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.name)
            .collect()
    } else {
        vec![]
    };

    let mut errors = Vec::new();

    // resolve group emails to individual members
    let mut resolved_recipients = Vec::new();
    for rcpt in &all_recipients {
        if let Some(ref ds) = state.domain_store {
            match ds.resolve_recipient(rcpt).await {
                crate::domain_store::ResolvedRecipient::Group(members) => {
                    resolved_recipients.extend(members);
                }
                _ => resolved_recipients.push(rcpt.clone()),
            }
        } else {
            resolved_recipients.push(rcpt.clone());
        }
    }

    // deduplicate recipients (e.g. user both in a group and directly CC'd)
    resolved_recipients.sort_unstable();
    resolved_recipients.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    // Pre-compute subject + snippet once so the NewMessage events we
    // emit below carry the same payload shape as the inbound-SMTP
    // path (smtp_session/events/data). Without these events, the
    // freshly-sent message was invisible until a manual list/thread
    // fetch — sometimes minutes — because no WS / JMAP push fired.
    let send_subject = crate::message_util::decode_header(
        &crate::message_util::extract_header_from_raw(raw, "Subject"),
    );
    let send_snippet: String = {
        let (text, _, _) = crate::message_util::parse_message(raw);
        text.unwrap_or_default().chars().take(200).collect()
    };
    let send_from_display = {
        let from_header = crate::message_util::extract_header_from_raw(raw, "From");
        if from_header.is_empty() {
            from.to_string()
        } else {
            crate::message_util::decode_header(&from_header)
        }
    };

    for rcpt in &resolved_recipients {
        let domain = rcpt.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        let is_local = local_domains
            .iter()
            .any(|d: &String| d.eq_ignore_ascii_case(domain));

        if is_local {
            if let Some(ref mb_store) = state.mailbox_store {
                let _ = mb_store.ensure_default_mailboxes(rcpt).await;
                if let Err(e) = crate::message_store::deliver_and_index(
                    state.message_store.as_ref(),
                    mb_store,
                    rcpt,
                    "INBOX",
                    &state.maildir_root,
                    raw,
                    0,
                    ts,
                )
                .await
                {
                    errors.push(format!("{rcpt}: {e}"));
                } else {
                    if let Some(ref vk) = state.kevy_embed {
                        // Bust the recipient's conversation caches so the
                        // newly-delivered local message shows up on their
                        // next thread/list fetch — without this, the cached
                        // thread list silently misses the message until TTL.
                        crate::conversation_cache::bust_user(vk, rcpt);
                    }
                    // Fire the same NewMessage event the SMTP inbound
                    // pipeline emits, so the recipient's IMAP IDLE / WS /
                    // JMAP-push subscribers all see the new mail right
                    // away instead of waiting for a manual refresh. Empty
                    // thread_id is acceptable here — the frontend uses
                    // the event as a "refresh" trigger; thread_id is only
                    // used to invalidate the open-thread cache when it
                    // matches, and we don't have it cheaply at this
                    // point. Per-message `bust_user` above covers the
                    // wider cache.
                    state
                        .event_bus
                        .emit(crate::event_bus::SmtpEvent::NewMessage {
                            user: rcpt.clone(),
                            thread_id: String::new(),
                            sender: send_from_display.clone(),
                            subject: send_subject.clone(),
                            snippet: send_snippet.clone(),
                        });
                }
            }
        } else if let Some(ref pool) = state.outbound_queue {
            let enqueue_result = if let Some(sched) = scheduled_at {
                mailrs_outbound_queue::queue::enqueue_scheduled(
                    pool,
                    from,
                    rcpt,
                    domain,
                    raw,
                    Some(message_id),
                    ts,
                    sched,
                )
                .await
            } else {
                mailrs_outbound_queue::queue::enqueue(
                    pool,
                    from,
                    rcpt,
                    domain,
                    raw,
                    Some(message_id),
                    ts,
                )
                .await
            };
            if let Err(e) = enqueue_result {
                errors.push(format!("{rcpt}: {e}"));
            } else if let Some(ref store) = state.kevy_embed {
                // in-process kevy notify — sync publish to the shared bus,
                // wakes the DeliveryWorker's Subscription::recv listener.
                mailrs_outbound_queue::queue::notify(store.as_ref());
            }
        } else {
            errors.push(format!("{rcpt}: outbound queue not configured"));
        }
    }

    // save copy to Sent folder
    if let Some(ref mb_store) = state.mailbox_store {
        let _ = mb_store.ensure_default_mailboxes(from).await;
        let sent_ok = crate::message_store::deliver_and_index(
            state.message_store.as_ref(),
            mb_store,
            from,
            "Sent",
            &state.maildir_root,
            raw,
            mailrs_mailbox::FLAG_SEEN,
            ts,
        )
        .await
        .is_ok();
        // Bust the sender's conversation caches so the newly-Sent
        // message shows up in the thread view on the next fetch.
        // Without this, multi-turn conversation replies appeared to
        // "succeed" (Sent folder gets the message, API returns 200) but
        // the cached thread list silently dropped them until TTL expiry.
        // Use `bust_user` since the per-thread cache key depends on the
        // mailbox-store-assigned thread_id which we don't have in this
        // scope yet; the wider bust is acceptable here because send is
        // a comparatively rare operation.
        if let Some(ref vk) = state.kevy_embed {
            crate::conversation_cache::bust_user(vk, from);
        }
        // And the missing other half: fire NewMessage on the bus so
        // the sender's own IMAP IDLE / WS / JMAP push subscribers
        // refresh their list right away. The previous code paths only
        // busted Kevy, which the frontend only consults on the next
        // explicit fetch — so a sent message sat invisible until the
        // user manually refreshed (the symptom: "Sent shows up after a
        // few minutes"). Emit the same shape the SMTP inbound pipeline
        // uses (`smtp_session/events/data/mod.rs`).
        if sent_ok {
            state
                .event_bus
                .emit(crate::event_bus::SmtpEvent::NewMessage {
                    user: from.to_string(),
                    thread_id: String::new(),
                    sender: send_from_display.clone(),
                    subject: send_subject.clone(),
                    snippet: send_snippet.clone(),
                });
        }
    }

    if errors.is_empty() {
        Json(SendResult {
            success: true,
            message: None,
            message_id: Some(message_id.to_string()),
        })
    } else {
        Json(SendResult {
            success: false,
            message: Some(errors.join("; ")),
            message_id: None,
        })
    }
}

// extract bare email from "Display Name <addr>" or return as-is
pub(super) fn extract_address(s: &str) -> String {
    if let Some(start) = s.rfind('<')
        && let Some(end) = s[start..].find('>')
    {
        return s[start + 1..start + end].trim().to_string();
    }
    s.trim().to_string()
}

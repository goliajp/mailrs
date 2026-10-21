//! Handing a composed message to its recipients: local mailboxes, the
//! outbound queue, and the sender's own Sent folder.

use std::sync::Arc;

use axum::Json;

use super::super::{SendResult, WebState};
use super::extract_address;

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

    let local_domains = local_domain_names(state).await;

    let mut errors = Vec::new();

    let resolved_recipients = resolve_group_recipients(state, &all_recipients).await;

    let notice = NewMessageNotice::from_raw(raw, from);

    for rcpt in &resolved_recipients {
        let domain = rcpt.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        let is_local = local_domains
            .iter()
            .any(|d: &String| d.eq_ignore_ascii_case(domain));

        let delivered = if is_local {
            deliver_locally(state, rcpt, raw, ts, &notice).await
        } else {
            enqueue_outbound(state, from, rcpt, domain, raw, message_id, ts, scheduled_at).await
        };
        if let Err(e) = delivered {
            errors.push(e);
        }
    }

    save_sent_copy(state, from, raw, ts, &notice).await;

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

async fn local_domain_names(state: &WebState) -> Vec<String> {
    if let Some(ref ds) = state.domain_store {
        ds.list_domains()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.name)
            .collect()
    } else {
        vec![]
    }
}

async fn resolve_group_recipients(state: &WebState, all_recipients: &[String]) -> Vec<String> {
    // resolve group emails to individual members
    // through the route delivery takes, so a group here is a group there; a
    // lookup that fails sends to the address as written
    let mut resolved_recipients = Vec::new();
    let dir = state.domain_store.clone().map(|ds| {
        std::sync::Arc::new(crate::intake::PgDirectory::new(
            ds,
            state.maildir_root.clone(),
            std::env::var("MAILRS_KEVY_URL").ok(),
        ))
    });
    for rcpt in all_recipients {
        let route = match &dir {
            Some(dir) => dir.clone().route(rcpt.clone()).await.ok(),
            None => None,
        };
        match route {
            Some(mailrs_intake::route::Route::Group(group)) => {
                resolved_recipients.extend(group.members);
            }
            _ => resolved_recipients.push(rcpt.clone()),
        }
    }

    // deduplicate recipients (e.g. user both in a group and directly CC'd)
    resolved_recipients.sort_unstable();
    resolved_recipients.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    resolved_recipients
}

/// What every NewMessage event for this send carries.
struct NewMessageNotice {
    sender: String,
    subject: String,
    snippet: String,
}

impl NewMessageNotice {
    fn from_raw(raw: &[u8], from: &str) -> Self {
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
        Self {
            sender: send_from_display,
            subject: send_subject,
            snippet: send_snippet,
        }
    }

    fn emit_to(&self, state: &WebState, user: String) {
        state
            .event_bus
            .emit(crate::event_bus::SmtpEvent::NewMessage {
                user,
                thread_id: String::new(),
                sender: self.sender.clone(),
                subject: self.subject.clone(),
                snippet: self.snippet.clone(),
            });
    }
}

async fn deliver_locally(
    state: &WebState,
    rcpt: &str,
    raw: &[u8],
    ts: i64,
    notice: &NewMessageNotice,
) -> Result<(), String> {
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
            return Err(format!("{rcpt}: {e}"));
        }
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
        notice.emit_to(state, rcpt.to_string());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn enqueue_outbound(
    state: &WebState,
    from: &str,
    rcpt: &str,
    domain: &str,
    raw: &[u8],
    message_id: &str,
    ts: i64,
    scheduled_at: Option<i64>,
) -> Result<(), String> {
    let Some(ref pool) = state.outbound_queue else {
        return Err(format!("{rcpt}: outbound queue not configured"));
    };
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
        mailrs_outbound_queue::queue::enqueue(pool, from, rcpt, domain, raw, Some(message_id), ts)
            .await
    };
    if let Err(e) = enqueue_result {
        return Err(format!("{rcpt}: {e}"));
    }
    if let Some(ref store) = state.kevy_embed {
        // in-process kevy notify — sync publish to the shared bus,
        // wakes the DeliveryWorker's Subscription::recv listener.
        mailrs_outbound_queue::queue::notify(store.as_ref());
    }
    Ok(())
}

async fn save_sent_copy(
    state: &WebState,
    from: &str,
    raw: &[u8],
    ts: i64,
    notice: &NewMessageNotice,
) {
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
            notice.emit_to(state, from.to_string());
        }
    }
}

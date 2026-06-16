//! Sieve script evaluation + side-effect handling per recipient.

use mailrs_mail_builder::MessageBuilder;
use mailrs_rfc5322::Message;
use mailrs_sieve::{SieveAction, compile_sieve, evaluate_sieve_with_envelope};

use super::super::DeliveryDeps;
use crate::AccountStore;

/// Evaluate sieve script for `rcpt` (if any) against `full_message`.
/// Returns `(rcpt_folder, skip_delivery)` — the destination folder
/// (after FileInto), and whether Discard/Reject was matched.
/// Side effects: enqueue Redirect/Vacation outbound messages.
pub async fn apply_sieve_actions(
    rcpt: &str,
    target_folder: &str,
    reverse_path: &str,
    full_message: &[u8],
    deps: &DeliveryDeps<'_>,
) -> (String, bool) {
    let mut rcpt_folder = target_folder.to_string();
    let mut skip_delivery = false;

    let Some(ds) = deps.account_store else {
        return (rcpt_folder, skip_delivery);
    };
    let Ok(Some(script)) = ds.sieve_script(rcpt).await else {
        return (rcpt_folder, skip_delivery);
    };
    let compiled = match compile_sieve(&script) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                event = "sieve_compile_error",
                user = rcpt,
                error = e.as_str(),
                "failed to compile sieve script"
            );
            return (rcpt_folder, skip_delivery);
        }
    };
    let actions =
        evaluate_sieve_with_envelope(&compiled, full_message, Some(reverse_path), Some(rcpt));
    for action in &actions {
        match action {
            SieveAction::Keep { .. } => {}
            SieveAction::FileInto { mailbox, .. } => {
                // TODO(ckpt 7): apply RFC 5232 imap4flags — flags carried
                // through the wrapper API but not yet persisted to storage.
                rcpt_folder = mailbox.clone();
            }
            SieveAction::Discard => {
                tracing::info!(
                    event = "sieve_discard",
                    user = rcpt,
                    "sieve discarded message"
                );
                skip_delivery = true;
            }
            SieveAction::Redirect(addr) => {
                enqueue_sieve_outbound(rcpt, reverse_path, addr, full_message, deps).await;
                tracing::info!(
                    event = "sieve_redirect",
                    user = rcpt,
                    target = addr.as_str(),
                    "sieve redirected message"
                );
            }
            SieveAction::Vacation { .. } => {
                handle_vacation(rcpt, reverse_path, full_message, action, ds.as_ref(), deps).await;
            }
            SieveAction::Reject(reason) => {
                tracing::info!(
                    event = "sieve_reject",
                    user = rcpt,
                    reason = reason.as_str(),
                    "sieve rejected message"
                );
                skip_delivery = true;
            }
        }
    }
    (rcpt_folder, skip_delivery)
}

/// Enqueue an outbound copy for sieve Redirect/Vacation actions.
/// Side effect: also notifies the outbound dispatcher via kevy.
async fn enqueue_sieve_outbound(
    _rcpt: &str,
    from: &str,
    to: &str,
    body: &[u8],
    deps: &DeliveryDeps<'_>,
) {
    let Some(queue) = deps.outbound_enqueue else {
        return;
    };
    let now = chrono::Utc::now().timestamp();
    let domain = to.split_once('@').map(|(_, d)| d).unwrap_or("unknown");
    let _ = queue
        .enqueue(from, to, domain, body, None, now, false)
        .await;
    if let Some(notifier) = deps.queue_notifier {
        notifier.notify().await;
    }
}

/// Handle a sieve `vacation` action: dedup per RFC 5230 §4.6, build the
/// auto-reply (RFC 5230 §4.2/4.3 defaults + RFC 3834 loop guard via
/// `Auto-Submitted`), enqueue it, and record the send.
async fn handle_vacation(
    rcpt: &str,
    reverse_path: &str,
    full_message: &[u8],
    action: &SieveAction,
    ds: &dyn AccountStore,
    deps: &DeliveryDeps<'_>,
) {
    let SieveAction::Vacation {
        reason,
        subject,
        from,
        handle,
        period_secs,
        ..
    } = action
    else {
        return;
    };

    // dedup handle: the script's `:handle`, else a stable hash of
    // subject + reason so distinct messages dedup independently.
    let handle_key = match handle {
        Some(h) => h.clone(),
        None => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            subject.as_deref().unwrap_or("").hash(&mut hasher);
            reason.hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        }
    };
    let period = (*period_secs).unwrap_or(7 * 86_400);

    match ds
        .should_send_vacation_reply(rcpt, reverse_path, &handle_key, period)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            tracing::info!(
                event = "sieve_vacation_suppressed",
                user = rcpt,
                "vacation reply suppressed within dedup window"
            );
            return;
        }
        Err(e) => {
            tracing::warn!(
                event = "sieve_vacation_dedup_error",
                user = rcpt,
                error = %e,
                "vacation dedup check failed; skipping reply"
            );
            return;
        }
    }

    let from_addr = from.as_deref().unwrap_or(rcpt);
    let subject_line = match subject {
        Some(s) => s.clone(),
        None => Message::new(full_message)
            .header_str("Subject")
            .map(|s| format!("Auto: {s}"))
            .unwrap_or_else(|| "Automated reply".to_string()),
    };
    // TODO: `:mime` true means `reason` is already a full MIME entity;
    // treated as plain text for now.
    let body = MessageBuilder::new()
        .from(from_addr)
        .to(reverse_path)
        .subject(subject_line)
        .header("Auto-Submitted", "auto-replied")
        .text_body(reason.as_str())
        .build();

    enqueue_sieve_outbound(rcpt, from_addr, reverse_path, &body, deps).await;
    let now = chrono::Utc::now().timestamp();
    if let Err(e) = ds
        .record_vacation_reply(rcpt, reverse_path, &handle_key, now)
        .await
    {
        tracing::warn!(
            event = "sieve_vacation_record_error",
            user = rcpt,
            error = %e,
            "failed to record vacation reply"
        );
    }
    tracing::info!(
        event = "sieve_vacation",
        user = rcpt,
        target = reverse_path,
        "sieve vacation auto-reply sent"
    );
}

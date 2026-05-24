//! Sieve script evaluation + side-effect handling per recipient.

use mailrs_sieve::{compile_sieve, evaluate_sieve_with_envelope, SieveAction};

use super::super::super::ConnectionContext;

/// Evaluate sieve script for `rcpt` (if any) against `full_message`.
/// Returns `(rcpt_folder, skip_delivery)` — the destination folder
/// (after FileInto), and whether Discard/Reject was matched.
/// Side effects: enqueue Redirect/Vacation outbound messages.
pub(super) async fn apply_sieve_actions(
    rcpt: &str,
    target_folder: &str,
    reverse_path: &str,
    full_message: &[u8],
    ctx: &ConnectionContext,
) -> (String, bool) {
    let mut rcpt_folder = target_folder.to_string();
    let mut skip_delivery = false;

    let Some(ref ds) = ctx.domain_store else {
        return (rcpt_folder, skip_delivery);
    };
    let Ok(Some(script)) = ds.get_sieve_script(rcpt).await else {
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
    let actions = evaluate_sieve_with_envelope(
        &compiled,
        full_message,
        Some(reverse_path),
        Some(rcpt),
    );
    for action in &actions {
        match action {
            SieveAction::Keep => {}
            SieveAction::FileInto(folder) => {
                rcpt_folder = folder.clone();
            }
            SieveAction::Discard => {
                tracing::info!(event = "sieve_discard", user = rcpt, "sieve discarded message");
                skip_delivery = true;
            }
            SieveAction::Redirect(addr) => {
                enqueue_sieve_outbound(rcpt, reverse_path, addr, full_message, ctx).await;
                tracing::info!(
                    event = "sieve_redirect",
                    user = rcpt,
                    target = addr.as_str(),
                    "sieve redirected message"
                );
            }
            SieveAction::Vacation(addr, reply_body) => {
                enqueue_sieve_outbound(rcpt, rcpt, addr, reply_body, ctx).await;
                tracing::info!(
                    event = "sieve_vacation",
                    user = rcpt,
                    target = addr.as_str(),
                    "sieve vacation auto-reply sent"
                );
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
/// Side effect: also notifies the outbound dispatcher via valkey.
async fn enqueue_sieve_outbound(
    _rcpt: &str,
    from: &str,
    to: &str,
    body: &[u8],
    ctx: &ConnectionContext,
) {
    let Some(ref pool) = ctx.outbound_queue else {
        return;
    };
    let now = chrono::Utc::now().timestamp();
    let domain = to.split_once('@').map(|(_, d)| d).unwrap_or("unknown");
    let _ = mailrs_outbound_queue::queue::enqueue(pool, from, to, domain, body, None, now).await;
    if let Some(ref vk) = ctx.valkey {
        mailrs_outbound_queue::queue::notify(&mut vk.clone()).await;
    }
}

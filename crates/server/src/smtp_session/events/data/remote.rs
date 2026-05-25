//! `enqueue_remote_rcpts` — push outbound recipients into the
//! outbound queue with SRS rewriting for forwards.

use crate::event_bus::SmtpEvent;

use super::super::super::ConnectionContext;
use super::super::super::srs::srs_rewrite;

pub(super) enum RemoteEnqueueResult {
    Ok,
    PartialFailure,
    RelayDenied,
}

/// Enqueue `remote_rcpts` into the outbound queue. Applies SRS
/// rewriting for forwarded entries. Returns:
/// - `RelayDenied` if any non-forwarded recipient is present and
///   the sender is not authenticated (caller responds 550).
/// - `PartialFailure` if any enqueue failed (caller sets `ok = false`).
/// - `Ok` otherwise.
pub(super) async fn enqueue_remote_rcpts(
    remote_rcpts: &[(String, bool)],
    reverse_path: &str,
    full_message: &[u8],
    is_authenticated: bool,
    conn_id: u64,
    ctx: &ConnectionContext,
) -> RemoteEnqueueResult {
    let has_user_remote = remote_rcpts.iter().any(|(_, fwd)| !fwd);
    if has_user_remote && !is_authenticated {
        return RemoteEnqueueResult::RelayDenied;
    }

    let Some(ref pool) = ctx.outbound_queue else {
        tracing::error!(
            event = "no_outbound_queue",
            "outbound queue unavailable, cannot relay"
        );
        return RemoteEnqueueResult::PartialFailure;
    };

    let now = chrono::Utc::now().timestamp();
    let mut enqueue_ok = false;
    let mut had_failure = false;
    for (rcpt, is_fwd) in remote_rcpts {
        let domain = rcpt.split_once('@').map(|(_, d)| d).unwrap_or("unknown");
        let envelope_sender = if *is_fwd && !reverse_path.is_empty() {
            if let Some(ref secret) = ctx.srs_secret {
                srs_rewrite(reverse_path, &ctx.hostname, secret)
            } else {
                reverse_path.to_string()
            }
        } else {
            reverse_path.to_string()
        };
        match mailrs_outbound_queue::queue::enqueue_ex(
            pool,
            &envelope_sender,
            rcpt,
            domain,
            full_message,
            None,
            now,
            *is_fwd,
        )
        .await
        {
            Ok(_) => enqueue_ok = true,
            Err(e) => {
                tracing::error!(
                    event = "enqueue_failed",
                    rcpt = rcpt,
                    error = %e,
                    "failed to enqueue remote recipient"
                );
                had_failure = true;
            }
        }
    }
    if enqueue_ok {
        if let Some(ref vk) = ctx.valkey {
            mailrs_outbound_queue::queue::notify(&mut vk.clone()).await;
        }
        ctx.event_bus.emit(SmtpEvent::MessageQueued {
            id: conn_id,
            from: reverse_path.to_string(),
            to: remote_rcpts.iter().map(|(a, _)| a.clone()).collect(),
        });
    }
    if had_failure {
        RemoteEnqueueResult::PartialFailure
    } else {
        RemoteEnqueueResult::Ok
    }
}

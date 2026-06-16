//! Spool-mode DATA acceptance (P6 receiver/core split).
//!
//! When `ConnectionContext::spool_sink` is set (the receiver binary), the DATA
//! handler hands off here right after antispam: the receiver does NOT resolve
//! recipients / run sieve / deliver / relay — it writes the accepted message
//! to the spool and emits a `SpoolDelivered` wake-up. The core consumer does
//! resolve/sieve/deliver/relay later (it owns spg). A coarse relay guard stays
//! here so the receiver never spools open-relay mail; fine-grained
//! forward/SRS stays in the core.

use futures_util::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_core::event_bus::SmtpEvent;
use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_smtp_codec::SmtpCodec;
use mailrs_smtp_proto::EnhancedCode;
use mailrs_smtp_proto::response::Response;

use super::super::super::{ConnectionContext, SessionAction};

/// Handle a DATA payload in spool mode: coarse relay guard → encode envelope +
/// body → write spool → emit `SpoolDelivered` → 250. Returns the next session
/// action. `full_message` is the antispam-processed body (Received header
/// already prepended). Only called when `ctx.spool_sink` is `Some`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_spool_mode<S>(
    framed: &mut Framed<S, SmtpCodec>,
    reverse_path: &str,
    forward_paths: &[String],
    is_authenticated: bool,
    conn_id: u64,
    target_folder: &str,
    full_message: &[u8],
    ctx: &ConnectionContext,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let Some(ref sink) = ctx.spool_sink else {
        // caller guarantees Some; defensive close otherwise
        return SessionAction::Close;
    };

    // Coarse open-relay guard: an unauthenticated session with any forward-path
    // outside our local domains is a relay attempt — reject the whole
    // transaction (matches the monolith's RelayDenied on enqueue_remote_rcpts).
    if !is_authenticated && has_remote_rcpt(forward_paths, &ctx.local_domains) {
        let resp = Response::new(
            550,
            Some(EnhancedCode {
                class: 5,
                subject: 7,
                detail: 1,
            }),
            "Relay access denied",
        );
        if framed.send(resp.format()).await.is_err() {
            return SessionAction::Close;
        }
        return SessionAction::Continue;
    }

    let received_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let env = SpoolEnvelope {
        reverse_path: reverse_path.to_string(),
        forward_paths: forward_paths.to_vec(),
        is_authenticated,
        conn_id,
        target_folder: target_folder.to_string(),
        received_at,
        schema_version: SPOOL_SCHEMA_VERSION,
    };
    let blob = encode_spool_blob(&env, full_message);

    let resp = match sink.write(&blob).await {
        Ok(spool_id) => {
            ctx.event_bus.emit(SmtpEvent::SpoolDelivered {
                spool_id,
                recipient_count: forward_paths.len(),
            });
            ctx.metrics.on_message_delivered();
            Response::data_ok()
        }
        Err(e) => {
            tracing::error!(
                event = "spool_write_failed",
                error = %e,
                "receiver spool write failed; message NOT accepted"
            );
            Response::new(
                451,
                Some(EnhancedCode {
                    class: 4,
                    subject: 3,
                    detail: 0,
                }),
                "Local error in processing",
            )
        }
    };
    if framed.send(resp.format()).await.is_err() {
        return SessionAction::Close;
    }
    SessionAction::Continue
}

/// True if any forward-path's domain is not in `local_domains` (case-insensitive).
fn has_remote_rcpt(forward_paths: &[String], local_domains: &[String]) -> bool {
    forward_paths.iter().any(|r| {
        r.rsplit_once('@')
            .map(|(_, d)| !local_domains.iter().any(|ld| ld.eq_ignore_ascii_case(d)))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::has_remote_rcpt;

    #[test]
    fn all_local_is_not_remote() {
        let local = vec!["smk.ai".to_string()];
        assert!(!has_remote_rcpt(
            &["a@smk.ai".into(), "b@smk.ai".into()],
            &local
        ));
    }

    #[test]
    fn any_remote_is_remote() {
        let local = vec!["smk.ai".to_string()];
        assert!(has_remote_rcpt(
            &["a@smk.ai".into(), "b@gmail.com".into()],
            &local
        ));
    }

    #[test]
    fn domain_match_is_case_insensitive() {
        let local = vec!["SMK.ai".to_string()];
        assert!(!has_remote_rcpt(&["a@smk.AI".into()], &local));
    }

    #[test]
    fn no_at_sign_is_not_remote() {
        // a bare token (no domain) can't be classified remote — leave to core
        let local = vec!["smk.ai".to_string()];
        assert!(!has_remote_rcpt(&["postmaster".into()], &local));
    }
}

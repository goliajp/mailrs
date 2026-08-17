//! Fastcore-native outbound SMTP sender.
//!
//! Drains `mailrs:outbound:pending` (RPOP → FIFO oldest first), resolves
//! MX for the recipient domain, connects on port 25, opportunistically
//! STARTTLS, and delivers the raw `message_data` blob via `MAIL FROM /
//! RCPT TO / DATA`. On 2xx it DELs the blob; on transient errors it
//! bumps the `attempts` counter and RPUSHes back to the tail with a
//! per-attempt sleep floor; on 5xx or attempts-exhausted it moves the
//! id into `mailrs:outbound:failed` for operator inspection.
//!
//! No spg. No monolith. Reads/writes only network kevy at
//! `MAILRS_KEVY_URL`.
//!
//! Env:
//!   MAILRS_KEVY_URL              — required, kevy://host:port
//!   MAILRS_HELO_HOSTNAME         — default "mail.golia.jp"
//!   MAILRS_SENDER_MAX_ATTEMPTS   — default 10
//!   MAILRS_SENDER_POLL_MS        — default 500 (idle sleep)
//!   MAILRS_SENDER_RETRY_MIN_SECS — default 60 (per-item minimum retry delay)

use std::sync::Arc;
use std::time::Duration;

use mailrs_outbound_queue::dkim_sign::DkimSignConfig;
use mailrs_smtp_client::{SmtpConnection, TimeoutConfig, TokioResolver, resolve_mx};
use tokio::task::spawn_blocking;

mod config;
mod deliver;
mod outcome;
mod queue;

use config::*;
use deliver::*;
use outcome::*;
use queue::*;

// v2.5.3 §P8-B-C: sender BRPOPs the v2 pending-idx now, not the
// legacy list. Enqueue still LPUSHes both (via core-sidestate) —
// Phase 8.2 drops the legacy write path. Duplicate ids in
// pending-idx (from Phase 6.2/7 LPUSH-without-RPOP semantics) are
// filtered by the WATCH+CAS state=pending check in `pop_next`.
/// The queue and the schedule, named by the crate that writes them.
///
/// Both were local copies of the strings here. The scheduled one had
/// drifted: this file said `mailrs:outbound:scheduled` while every
/// writer, the cancel route and the MCP listing all said
/// `mailrs:outbound:scheduled-idx`, so the due-sweep walked a zset
/// nothing has written since v2.5.3 and no scheduled send was ever
/// promoted. Importing removes the second copy rather than correcting
/// it.
use mailrs_core_sidestate::families::outbound::{PENDING_IDX, SCHEDULED_IDX};

const PENDING_IDX_KEY: &[u8] = PENDING_IDX;
const FAILED_KEY: &[u8] = b"mailrs:outbound:failed";

impl Cfg {
    fn from_env() -> Self {
        Self {
            kevy_url: std::env::var("MAILRS_KEVY_URL")
                .expect("MAILRS_KEVY_URL required (kevy://host:port)"),
            helo: std::env::var("MAILRS_HELO_HOSTNAME")
                .unwrap_or_else(|_| "mail.golia.jp".to_string()),
            dsn_from_domain: mailrs_fastcore::bounce::dsn_identity().1,
            max_attempts: std::env::var("MAILRS_SENDER_MAX_ATTEMPTS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            poll_ms: std::env::var("MAILRS_SENDER_POLL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500),
            retry_min_secs: std::env::var("MAILRS_SENDER_RETRY_MIN_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(60),
            dkim: load_dkim_from_env(),
            arc_key: load_arc_key(),
        }
    }
}

fn kevy(url: &str) -> std::io::Result<kevy_client::Connection> {
    kevy_client::Connection::connect(url).map_err(std::io::Error::from)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Move scheduled outbound whose send time has arrived into pending.
/// The scheduled zset is score-ordered by send-at epoch, so we walk
/// from the front and stop at the first future item.
const SCHEDULED_KEY: &[u8] = SCHEDULED_IDX;

/// Process one dequeued id. Never panics — logs everything.
async fn process_one(cfg: Cfg, id: String) {
    let envelope = match load_envelope(cfg.clone(), id.clone()).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            tracing::warn!(%id, "blob missing, dropped from pending");
            return;
        }
        Err(e) => {
            tracing::error!(%id, err = %e, "load envelope failed, requeue-and-hope");
            // requeue as-is with a synthetic envelope containing just id
            let filler = serde_json::json!({"id": id, "attempts": 1, "last_error": e.to_string()});
            if let Err(e2) = requeue(cfg.clone(), id.clone(), filler).await {
                tracing::error!(%id, err = %e2, "requeue after load failure also failed");
            }
            return;
        }
    };

    let sender = envelope
        .get("sender")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let recipient = envelope
        .get("recipient")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Read the raw RFC 5322 bytes. Prefer message_data_b64 so 8-bit
    // MIME (binary attachments, non-UTF-8 encodings) survives the
    // JSON round-trip; fall back to the legacy plaintext field for
    // backwards compatibility with in-flight items enqueued before
    // the base64 switch.
    let message_bytes: Vec<u8> =
        if let Some(b64) = envelope.get("message_data_b64").and_then(|v| v.as_str()) {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .unwrap_or_default()
        } else {
            envelope
                .get("message_data")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .as_bytes()
                .to_vec()
        };
    let attempts_prev = envelope
        .get("attempts")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let last_attempt_at = envelope
        .get("last_attempt_at")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    if sender.is_empty() || recipient.is_empty() || message_bytes.is_empty() {
        tracing::error!(%id, "envelope malformed (missing sender/recipient/message_data)");
        let _ = move_to_failed(cfg, id, "malformed envelope".into(), true).await;
        return;
    }

    // Enforce a soft per-item retry floor. If we picked this up too
    // soon, put it back and let another item run first.
    let now = now_secs();
    if attempts_prev > 0 && (now - last_attempt_at) < cfg.retry_min_secs {
        tracing::debug!(%id, attempts_prev, "retry floor not reached, requeuing");
        let _ = requeue(cfg.clone(), id, envelope).await;
        // Sleep briefly so the loop doesn't spin on a single retry-floor item.
        tokio::time::sleep(Duration::from_millis(cfg.poll_ms.max(500))).await;
        return;
    }

    tracing::info!(%id, %sender, %recipient, attempt = attempts_prev + 1, "delivering");
    let original_sender = envelope
        .get("original_sender")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let outcome = try_deliver(&cfg, &sender, &recipient, &message_bytes, &original_sender).await;
    // Once, before the queue handling below, so every arm is covered by
    // construction rather than by remembering.
    report_send_outcome(
        &cfg,
        envelope.get("send_id").and_then(|v| v.as_str()),
        &sender,
        &recipient,
        &outcome,
        attempts_prev + 1,
    );
    match outcome {
        Outcome::Delivered => {
            // Relationship fact: the user has now sent to this address.
            // This is the only writer of `sent_count`, and without it
            // `is_mutual` / `has_sent_to` — the strongest inbound
            // importance signals — can never become true
            // (RFC 20260721-self-hosted-importance-ranking).
            record_sent_relationship(&cfg, &sender, &recipient);
            if let Err(e) = drop_blob(cfg, id.clone()).await {
                tracing::error!(%id, err = %e, "drop_blob after success failed");
            } else {
                tracing::info!(%id, "delivered + blob dropped");
            }
        }
        Outcome::Permanent(reason) => {
            tracing::warn!(%id, reason = %reason, "permanent — moving to failed");
            record_suppression(&cfg, &recipient, &reason);
            mailrs_fastcore::live_sync::audit_system("mail.send_failed", &recipient, &reason);
            enqueue_bounce_dsn(&cfg, &sender, &recipient, &reason, &message_bytes).await;
            if let Err(e) = move_to_failed(cfg, id.clone(), reason, true).await {
                tracing::error!(%id, err = %e, "move_to_failed after permanent failed");
            }
        }
        Outcome::Transient(reason) => {
            let attempts = attempts_prev + 1;
            if attempts >= cfg.max_attempts {
                tracing::warn!(
                    %id,
                    attempts,
                    reason = %reason,
                    "max attempts reached — moving to failed"
                );
                enqueue_bounce_dsn(&cfg, &sender, &recipient, &reason, &message_bytes).await;
                let _ = move_to_failed(cfg, id, reason, true).await;
                return;
            }
            let mut env = envelope;
            env["attempts"] = serde_json::Value::from(attempts);
            env["last_error"] = serde_json::Value::from(reason.clone());
            env["last_attempt_at"] = serde_json::Value::from(now_secs());
            tracing::info!(%id, attempts, %reason, "transient — requeue tail");
            if let Err(e) = requeue(cfg, id, env).await {
                tracing::error!(err = %e, "requeue after transient failed");
            }
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    // Install the process-wide rustls crypto provider before any TLS
    // config is built (STARTTLS in try_deliver). Without this rustls
    // 0.23 panics on first use — same fix mailrs-receiver / mailrs-server
    // apply. .ok() because a second install is a no-op error we can
    // safely ignore in a single-binary process.
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    let cfg = Arc::new(Cfg::from_env());
    tracing::info!(
        helo = %cfg.helo,
        max_attempts = cfg.max_attempts,
        poll_ms = cfg.poll_ms,
        retry_min_secs = cfg.retry_min_secs,
        "mailrs-fastcore-sender starting"
    );

    // Fail fast on kevy connect so misconfig surfaces at boot.
    if let Err(e) = kevy(&cfg.kevy_url) {
        tracing::error!(err = %e, "kevy connect failed at boot — exiting");
        std::process::exit(2);
    }

    // v2.5.2 §P8-B-B boot hook: reset any inflight job hash that a prior
    // sender crashed on. Best-effort — a network kevy blip here shouldn't
    // block the boot loop, it just means the recovery happens on the
    // periodic sweep instead.
    match recover_stale((*cfg).clone()).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(recovered = n, "recover_stale at boot"),
        Err(e) => tracing::warn!(err = %e, "recover_stale at boot failed"),
    }

    // BRPOP timer bound. Short enough to promote_due at cadence, long
    // enough that idle brpop dominates wall-clock over the wake-up +
    // reissue overhead. 5 s makes a due-sweep miss at most 5 s from
    // wall-clock — the sender loop's scheduled-sweep resolution.
    // v2.2 §2 (2026-07-08): supersedes MAILRS_SENDER_POLL_MS as the
    // idle-cadence knob (poll_ms retained only for error backoff).
    let brpop_wait = Duration::from_secs(5);
    let mut consecutive_errors: u32 = 0;
    let mut last_stale_sweep = now_secs();
    loop {
        // v2.5.2 §P8-B-B: periodic recover_stale (every 60 s). Cheap
        // when the queue is small; guards against sender crashes that
        // happen between boots and leave orphan inflight jobs.
        if now_secs() - last_stale_sweep >= 60 {
            if let Err(e) = recover_stale((*cfg).clone()).await {
                tracing::warn!(err = %e, "periodic recover_stale failed");
            }
            last_stale_sweep = now_secs();
        }
        // promote any scheduled sends whose time has arrived (G13)
        if let Err(e) = promote_due((*cfg).clone()).await {
            tracing::warn!(err = %e, "scheduled due-sweep failed");
        }
        match pop_next((*cfg).clone(), brpop_wait).await {
            Ok(Some(id)) => {
                consecutive_errors = 0;
                process_one((*cfg).clone(), id).await;
            }
            Ok(None) => {
                // BRPOP timer expired with queue empty. Loop back
                // immediately — no sleep needed; the next brpop() will
                // itself block for up to `brpop_wait`.
            }
            Err(e) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                let back_ms = (cfg.poll_ms * (1 << consecutive_errors.min(6))).min(30_000);
                tracing::error!(err = %e, back_ms, "pop_next error — backing off");
                tokio::time::sleep(Duration::from_millis(back_ms)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_addr_spec, extract_code, is_forward, is_remote_hard_bounce, signed_d_tag};

    /// The code is pulled out of the remote's own sentence, which is
    /// rarely code-first. Same reason `is_remote_hard_bounce` needed a
    /// token scan rather than `starts_with('5')`.
    #[test]
    fn a_code_is_found_anywhere_in_the_remotes_sentence() {
        assert_eq!(
            extract_code("gmail-smtp-in.l.google.com 550 5.1.1 does not exist."),
            550
        );
        assert_eq!(extract_code("451 4.7.1 greylisted, try later"), 451);
        // 5.1.1 is an enhanced status code, not a reply code, and must
        // not be mistaken for one.
        assert_eq!(extract_code("mx.example.com said: 5.1.1 unknown"), 0);
        assert_eq!(extract_code("connection timed out"), 0);
    }

    #[test]
    fn detects_a_5xx_inside_a_real_reason_string() {
        // Verbatim from a live rejection — the code is the second token,
        // which is why a starts_with('5') check found nothing.
        assert!(is_remote_hard_bounce(
            "gmail-smtp-in.l.google.com 550 5.1.1 The email account that you tried to reach does not exist."
        ));
        assert!(is_remote_hard_bounce(
            "mx.example.com 552 message too large"
        ));
    }

    #[test]
    fn ignores_transient_and_self_inflicted_failures() {
        assert!(!is_remote_hard_bounce("mx.example.com 450 try again later"));
        assert!(!is_remote_hard_bounce("invalid recipient: not-an-address"));
        assert!(!is_remote_hard_bounce("dkim sign: key unreadable"));
        assert!(!is_remote_hard_bounce(
            "recipient on suppression list: a@b.c"
        ));
        assert!(!is_remote_hard_bounce("no MX for example.com"));
    }

    /// **A 5xx about us is not a bad address.**
    ///
    /// 2026-08-17: Microsoft answered `550 5.7.1 Unfortunately, messages
    /// from [52.195.89.111] weren't sent … part of their network is on
    /// our block list (S3140)` — a statement about our egress IP. It was
    /// recorded as a hard bounce against the *recipient*, who was
    /// suppressed for 90 days. So after the block is lifted, mail to
    /// that person would still be silently dropped for three months, by
    /// us, for a reason that was never about them.
    ///
    /// RFC 3463 says which: the second sub-code is the subject. `5.1.x`
    /// is address status and `5.2.x` is mailbox status — those are the
    /// recipient. `5.7.x` is security and policy, `5.3.x` the receiving
    /// system, `5.4.x` routing. None of those say the address is bad.
    #[test]
    fn a_policy_rejection_is_not_the_recipients_fault() {
        assert!(
            !is_remote_hard_bounce(
                "hotmail-com.olc.protection.outlook.com 550 5.7.1 Unfortunately, messages \
                 from [52.195.89.111] weren't sent. Please contact your Internet service \
                 provider since part of their network is on our block list (S3140)."
            ),
            "an IP-reputation block suppressed the recipient it was not about"
        );
        // The recipient's own problems still count, and this is the
        // shape that makes the list worth having.
        assert!(is_remote_hard_bounce(
            "gmail-smtp-in.l.google.com 550 5.1.1 The email account that you tried to reach does not exist."
        ));
        assert!(is_remote_hard_bounce(
            "mx.example.com 550 5.2.1 mailbox disabled"
        ));
        // Other subjects: the receiving system, routing, content. None
        // of them is evidence about the address.
        for reason in [
            "mx.example.com 550 5.3.2 system not accepting network messages",
            "mx.example.com 550 5.4.1 no answer from host",
            "mx.example.com 550 5.7.26 unauthenticated email is not accepted",
        ] {
            assert!(!is_remote_hard_bounce(reason), "{reason}");
        }
        // And a bare 5xx with no enhanced code keeps its old meaning:
        // `550 no such user` is why the list exists, and the remote told
        // us nothing more precise.
        assert!(is_remote_hard_bounce("mx.example.com 550 no such user"));
    }

    #[test]
    fn an_enhanced_code_alone_does_not_count() {
        // 5.1.1 is not a three-digit token, so a reason carrying only
        // an enhanced code must not be read as a rejection.
        assert!(!is_remote_hard_bounce("failed with status 5.1.1"));
    }

    #[test]
    fn a_rewritten_envelope_sender_marks_a_forward() {
        assert!(is_forward(
            "SRS0=abc=xy=other.com=user@golia.jp",
            "user@other.com"
        ));
    }

    #[test]
    fn an_ordinary_send_is_not_a_forward() {
        assert!(!is_forward("a@golia.jp", "a@golia.jp"));
        assert!(!is_forward("A@Golia.JP", "a@golia.jp"));
        assert!(!is_forward("<a@golia.jp>", "a@golia.jp"));
    }

    #[test]
    fn system_mail_is_not_a_forward() {
        // DSNs enqueue with the null sender on both sides.
        assert!(!is_forward("<>", "<>"));
        assert!(!is_forward("", ""));
        assert!(!is_forward("a@golia.jp", ""));
    }

    #[test]
    fn reads_the_d_tag_from_a_signature() {
        let msg = b"DKIM-Signature: v=1; a=rsa-sha256; d=bitreits.com; s=mail;\r\n\
                    \tbh=abc; b=xyz\r\nFrom: a@b.c\r\n\r\nbody";

        assert_eq!(signed_d_tag(msg).as_deref(), Some("bitreits.com"));
    }

    #[test]
    fn reads_the_d_tag_across_a_folded_header() {
        let msg = b"DKIM-Signature: v=1; a=rsa-sha256;\r\n\
                    \ts=mail; d=doracawl.com; bh=abc;\r\n\tb=xyz\r\nFrom: a@b.c\r\n\r\nbody";

        assert_eq!(signed_d_tag(msg).as_deref(), Some("doracawl.com"));
    }

    #[test]
    fn stops_at_the_end_of_the_signature_header() {
        // A `d=` later in the message must not be picked up.
        let msg = b"DKIM-Signature: v=1; a=rsa-sha256; s=mail; b=xyz\r\n\
                    From: a@b.c\r\n\r\nbody with d=not-a-domain; in it";

        assert_eq!(signed_d_tag(msg), None);
    }

    #[test]
    fn unsigned_message_has_no_tag() {
        assert_eq!(signed_d_tag(b"From: a@b.c\r\n\r\nbody"), None);
        assert_eq!(signed_d_tag(b""), None);
    }

    #[test]
    fn bare_addr_spec_passes_through() {
        assert_eq!(
            extract_addr_spec("nagata@nagatax.tokyo.jp"),
            "nagata@nagatax.tokyo.jp"
        );
    }

    #[test]
    fn name_addr_extracts_inside_brackets() {
        assert_eq!(
            extract_addr_spec("Masato Nagata <nagata@nagatax.tokyo.jp>"),
            "nagata@nagatax.tokyo.jp"
        );
    }

    #[test]
    fn quoted_display_name_supported() {
        assert_eq!(
            extract_addr_spec("\"Nagata, M.\" <nagata@nagatax.tokyo.jp>"),
            "nagata@nagatax.tokyo.jp"
        );
    }

    #[test]
    fn trims_outer_whitespace() {
        assert_eq!(extract_addr_spec("  a@b.c  "), "a@b.c");
        assert_eq!(extract_addr_spec("  A <a@b.c>  "), "a@b.c");
    }
}

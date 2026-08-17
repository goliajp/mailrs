//! What to do with the answer the remote MX gave.
//!
//! A 5xx is not the end of the story: a hard bounce has to reach the
//! sender as a DSN, go on the suppression list, and not be counted as a
//! relationship. Those three were interleaved with the SMTP conversation
//! itself until 2026-08-02.

use super::config::*;
use super::*;

/// Put a permanently-failed recipient on the suppression list.
///
/// Only genuine 5xx replies count. `Outcome::Permanent` also covers our
/// own refusals — malformed recipients, signing failures, and the
/// suppression check itself — and none of those are evidence about the
/// remote mailbox. See [`is_remote_hard_bounce`] for how the two are
/// told apart.
///
/// Best-effort: a message that already failed must not fail louder
/// because the side-state write did not land.
pub(super) fn record_suppression(cfg: &Cfg, recipient: &str, reason: &str) {
    use mailrs_core_sidestate::families::suppression;

    if !is_remote_hard_bounce(reason) {
        return;
    }
    let Ok(mut conn) = kevy(&cfg.kevy_url) else {
        tracing::warn!(%recipient, "suppression: no kevy connection");
        return;
    };
    match suppression::add(
        &mut conn,
        recipient,
        suppression::Source::HardBounce,
        reason,
        now_secs(),
    ) {
        Ok(()) => tracing::info!(
            %recipient,
            "suppressed after hard bounce (expires in 90 days)"
        ),
        Err(e) => tracing::warn!(error = %e, %recipient, "suppression: add failed"),
    }
}

/// Record "this user has sent to this address" on the shared contact
/// hash. Best-effort: a delivery that already succeeded must never be
/// failed or retried because a derived counter could not be bumped.
pub(super) fn record_sent_relationship(cfg: &Cfg, sender: &str, recipient: &str) {
    let from = sender.trim().to_lowercase();
    let to = recipient.trim().to_lowercase();
    if from.is_empty() || to.is_empty() {
        return;
    }
    let Ok(mut conn) = kevy(&cfg.kevy_url) else {
        tracing::warn!(%from, %to, "sent-relationship: no kevy connection");
        return;
    };
    if let Err(e) = mailrs_core_sidestate::families::contacts::record_sent_to(&mut conn, &from, &to)
    {
        tracing::warn!(error = %e, %from, %to, "sent-relationship: hincrby failed");
    }
}

/// Compose an RFC 3464 DSN for a permanently failed delivery and push
/// it onto the bounce hand-off queue fastcore drains (G9). Null /
/// daemon senders are suppressed — a bounce never bounces.
pub(super) async fn enqueue_bounce_dsn(
    cfg: &Cfg,
    sender: &str,
    recipient: &str,
    reason: &str,
    message: &[u8],
) {
    use base64::Engine as _;
    if mailrs_fastcore::bounce::suppress_bounce(sender) {
        tracing::info!(%recipient, "bounce suppressed (null/daemon sender)");
        return;
    }
    let dsn = mailrs_fastcore::bounce::compose_dsn(
        &cfg.helo,
        &cfg.dsn_from_domain,
        sender.trim_matches(|c| c == '<' || c == '>'),
        recipient,
        "5.0.0",
        reason,
        message,
    );
    let cfg = cfg.clone();
    let sender = sender.trim_matches(|c| c == '<' || c == '>').to_string();
    let res = spawn_blocking(move || {
        let mut c = kevy(&cfg.kevy_url)?;
        let id = format!("{}-{}", now_secs(), std::process::id());
        let key = format!("mailrs:bounce:{id}");
        let b64 = base64::engine::general_purpose::STANDARD.encode(&dsn);
        c.hset(
            key.as_bytes(),
            &[
                (b"recipient" as &[u8], sender.as_bytes()),
                (b"blob", b64.as_bytes()),
            ],
        )?;
        c.lpush(mailrs_fastcore::bounce::BOUNCE_PENDING, &[id.as_bytes()])?;
        Ok::<(), std::io::Error>(())
    })
    .await;
    match res {
        Ok(Ok(())) => {}
        other => tracing::warn!(?other, "bounce enqueue failed"),
    }
}

/// Delivery outcome per RCPT.
pub(super) enum Outcome {
    /// 2xx on DATA final response.
    Delivered,
    /// 4xx anywhere in the exchange, or a network error — retry.
    Transient(String),
    /// 5xx anywhere in the exchange — do not retry.
    Permanent(String),
}

/// Report one recipient's outcome onto the Send row (RFC
/// 20260730-send-status S2).
///
/// Called once, before the outcome is matched for queue handling, so
/// every arm is covered by construction. The match here is exhaustive:
/// a new `Outcome` variant fails to compile until it decides what the
/// user should see, which is the point — the previous design let the
/// view learn about a send from a periodic sweep, and a variant that
/// forgot to report would put us back there.
///
/// The remote's code and text are stored verbatim. When a send fails,
/// the receiving server's own words are the only useful thing on the
/// screen.
pub(super) fn report_send_outcome(
    cfg: &Cfg,
    send_id: Option<&str>,
    user: &str,
    recipient: &str,
    outcome: &Outcome,
    attempts: u32,
) {
    use mailrs_core_sidestate::families::send as sendfam;
    let Some(send_id) = send_id else {
        // Mail nobody composed — tls-rpt, a bounce DSN, a sieve
        // redirect. There is no Send row to report against, and
        // inventing one would put mail in the user's Send list that they
        // never wrote.
        return;
    };
    let state = match outcome {
        Outcome::Delivered => sendfam::RecipientState {
            recipient: recipient.to_string(),
            delivered: true,
            pending: false,
            code: 250,
            message: String::new(),
        },
        Outcome::Permanent(reason) => sendfam::RecipientState {
            recipient: recipient.to_string(),
            delivered: false,
            pending: false,
            code: extract_code(reason),
            message: reason.clone(),
        },
        Outcome::Transient(reason) => sendfam::RecipientState {
            recipient: recipient.to_string(),
            delivered: false,
            // Retries left keeps the whole send `sending`; only the
            // final attempt is a verdict. Publishing "failed" while a
            // retry is pending invites resending mail that is about to
            // arrive.
            pending: attempts < cfg.max_attempts,
            code: extract_code(reason),
            message: reason.clone(),
        },
    };
    let url = cfg.kevy_url.clone();
    let user = user.to_string();
    let send_id = send_id.to_string();
    if let Err(e) = (|| -> std::io::Result<()> {
        let mut c = kevy(&url)?;
        sendfam::update_recipient(&mut c, &user, &send_id, &state)?;
        Ok(())
    })() {
        tracing::warn!(err = %e, %send_id, %recipient, "send row outcome write failed");
    }
}

/// Pull a three-digit SMTP code out of a reason string, or 0.
///
/// The reason is the remote's text as we received it; the code is
/// duplicated into its own field so a UI can colour by class without
/// parsing prose, while the prose stays intact.
pub(super) fn extract_code(reason: &str) -> u16 {
    for token in reason.split_whitespace() {
        let digits: String = token.chars().take_while(char::is_ascii_digit).collect();
        if digits.len() == 3
            && let Ok(code) = digits.parse::<u16>()
            && (200..=599).contains(&code)
        {
            return code;
        }
    }
    0
}

/// Whether a failure reason carries a remote 5xx rejection.
///
/// `outbound_queue::is_hard_bounce` tests whether the string *starts*
/// with a 5, which suits a bare SMTP reply. Our reasons are assembled
/// as `"{mx} {code} {text}"`, so the code never leads and that check
/// silently returned false for every real rejection — the suppression
/// list stayed empty through a live 550. Scan the tokens instead.
///
/// Looking for a standalone three-digit 5xx also excludes the failures
/// we generate ourselves — "invalid recipient", "dkim sign", the
/// suppression check — none of which say anything about the remote
/// mailbox. Enhanced codes like `5.1.1` are not three digits, so they
/// cannot trigger it either.
/// And a 5xx is only evidence about the **recipient** when the remote
/// says so. RFC 3463's second sub-code names the subject: `5.1.x` is
/// address status, `5.2.x` mailbox status — those are the person we are
/// writing to. `5.7.x` is security and policy, `5.3.x` the receiving
/// system, `5.4.x` routing, `5.6.x` content. None of those is a bad
/// address, and suppressing on them punishes the recipient for
/// something on our side.
///
/// 2026-08-17: Microsoft answered `550 5.7.1 … part of their network is
/// on our block list (S3140)` about our egress IP, and the recipient was
/// suppressed for 90 days. After a delisting, mail to that person would
/// still have been dropped silently by us, for three months, for a
/// reason that was never about them.
///
/// A bare 5xx with no enhanced code keeps its old meaning: `550 no such
/// user` is why this list exists, and the remote told us nothing more
/// precise to go on.
pub(super) fn is_remote_hard_bounce(reason: &str) -> bool {
    let has_5xx = reason.split_whitespace().any(|token| {
        token.len() == 3 && token.starts_with('5') && token.bytes().all(|b| b.is_ascii_digit())
    });
    if !has_5xx {
        return false;
    }
    match enhanced_subject(reason) {
        Some(subject) => subject == 1 || subject == 2,
        None => true,
    }
}

/// The middle number of an RFC 3463 enhanced status code in a 5xx
/// reason — the one that says what the failure is *about*. `None` when
/// the remote sent no enhanced code.
fn enhanced_subject(reason: &str) -> Option<u16> {
    reason.split_whitespace().find_map(|token| {
        let t = token.trim_end_matches(|c: char| !c.is_ascii_digit());
        let mut parts = t.split('.');
        let class = parts.next()?;
        let subject = parts.next()?;
        let detail = parts.next()?;
        if class != "5" || subject.is_empty() || detail.is_empty() {
            return None;
        }
        if !subject.bytes().all(|b| b.is_ascii_digit())
            || !detail.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        subject.parse().ok()
    })
}

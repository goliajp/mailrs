//! Delivering one message: MX resolution, the SMTP conversation, ARC and
//! DKIM, and what to record about how it went.

use super::config::*;
use super::*;

/// ARC-seal `message` when this delivery is a forward, else `None`.
///
/// Returns `None` on every non-applicable or failing path — an unsealed
/// forward still goes out. See `arc_seal`'s module docs.
pub(super) fn arc_seal_target(
    cfg: &Cfg,
    sender: &str,
    original_sender: &str,
    message: &[u8],
) -> Option<Vec<u8>> {
    if !is_forward(sender, original_sender) {
        return None;
    }
    let key = cfg.arc_key.as_ref()?;
    let dkim = cfg.dkim.as_ref()?;
    mailrs_fastcore::arc_seal::seal_forwarded(message, key, &dkim.domain, &dkim.selector)
}

/// Whether this delivery is a forward rather than an original send.
///
/// The envelope sender is rewritten (SRS) when we forward on someone
/// else's behalf, so it stops matching the original. Equal values — and
/// the null sender on both sides, which is what system mail uses — mean
/// this is our own message.
pub(super) fn is_forward(sender: &str, original_sender: &str) -> bool {
    let s = sender.trim().trim_matches(['<', '>']);
    let o = original_sender.trim().trim_matches(['<', '>']);
    !o.is_empty() && !s.is_empty() && !s.eq_ignore_ascii_case(o)
}

/// Pull the `d=` tag out of the DKIM-Signature header that signing just
/// prepended.
///
/// Diagnostics only — a `None` never gates anything. Scans a bounded
/// prefix because the signature is prepended, so the header is always
/// at the very start; that also keeps the tag split from wandering into
/// the body, where semicolons are just bytes.
pub(super) fn signed_d_tag(signed: &[u8]) -> Option<String> {
    let head = &signed[..signed.len().min(1024)];
    let text = String::from_utf8_lossy(head);
    let sig = text.strip_prefix("DKIM-Signature:")?;
    // Header ends at the first bare CRLF (a folded continuation line
    // starts with whitespace, so it is not a terminator).
    let header_end = sig
        .match_indices("\r\n")
        .find(|(i, _)| !sig[i + 2..].starts_with([' ', '\t']))
        .map(|(i, _)| i)
        .unwrap_or(sig.len());
    for tag in sig[..header_end].split(';') {
        if let Some(v) = tag.trim().strip_prefix("d=") {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Extract the addr-spec from an RFC 5322 mailbox token.
///
/// Accepts both `addr@domain` (bare `addr-spec`) and
/// `Display Name <addr@domain>` (bare `name-addr`) forms. Trims
/// surrounding whitespace. Never panics — returns the original trimmed
/// input on parse failure so the caller can surface a
/// `Permanent(invalid recipient …)` error one level up.
pub(super) fn extract_addr_spec(raw: &str) -> &str {
    let t = raw.trim();
    if let Some(start) = t.rfind('<')
        && let Some(end) = t.rfind('>')
        && end > start
    {
        return t[start + 1..end].trim();
    }
    t
}

/// Attempt SMTP delivery via the recipient's MX hosts, in priority
/// order. Returns the first non-transient outcome; on all-transient
/// exhaustion returns `Outcome::Transient` with the last error.
pub(super) async fn try_deliver(
    cfg: &Cfg,
    sender: &str,
    recipient_raw: &str,
    message: &[u8],
    original_sender: &str,
) -> Outcome {
    let recipient = extract_addr_spec(recipient_raw);
    let sender = extract_addr_spec(sender);

    // ARC-seal first, so the DKIM signature covers a message that
    // already carries its ARC set.
    //
    // A forward is where the envelope sender differs from the original
    // sender — the SRS rewrite in enqueue_redirect produces exactly
    // that. Ordinary sends have the two equal, and system mail has both
    // as the null sender, so neither gets sealed.
    let sealed_msg;
    let message: &[u8] = match arc_seal_target(cfg, sender, original_sender, message) {
        Some(bytes) => {
            sealed_msg = bytes;
            &sealed_msg
        }
        None => message,
    };

    // DKIM sign if a signing key is configured. Fatal signing errors
    // are permanent (won't heal on retry): message data is malformed
    // or the private key is broken.
    let signed;
    let payload: &[u8] = match cfg.dkim.as_ref() {
        Some(dkim) => match dkim.sign(message) {
            Ok(bytes) => {
                signed = bytes;
                // Log the d= we actually signed with. Alignment failures
                // are invisible from this side — the message leaves
                // fine and only fails at the far end, on a forward,
                // weeks later. Recording the tag makes the question
                // "did this domain sign as itself?" answerable from
                // logs instead of from a round trip through a third
                // party's inbox.
                if let Some(d) = signed_d_tag(&signed) {
                    tracing::info!(dkim_d = %d, %sender, "signed");
                }
                &signed
            }
            Err(e) => {
                return Outcome::Permanent(format!("dkim sign: {e}"));
            }
        },
        None => message,
    };
    let Some(domain) = recipient.split('@').nth(1) else {
        return Outcome::Permanent(format!("invalid recipient: {recipient_raw}"));
    };
    if domain.is_empty() || domain.contains(char::is_whitespace) {
        return Outcome::Permanent(format!("invalid recipient: {recipient_raw}"));
    }

    // Suppression check. Repeatedly delivering to an address that hard-
    // bounced or complained is what costs sending reputation, so this
    // runs before any DNS or connection work. The reason string
    // deliberately does not start with a 5xx code — see
    // record_suppression, which would otherwise re-suppress on the way
    // back out.
    if let Ok(mut conn) = kevy(&cfg.kevy_url)
        && mailrs_core_sidestate::families::suppression::is_suppressed(&mut conn, recipient)
    {
        tracing::info!(%recipient, "suppressed recipient — not delivering");
        return Outcome::Permanent(format!("recipient on suppression list: {recipient}"));
    }

    let resolver = match TokioResolver::builder_tokio() {
        Ok(b) => match b.build() {
            Ok(r) => r,
            Err(e) => return Outcome::Transient(format!("resolver build: {e}")),
        },
        Err(e) => return Outcome::Transient(format!("resolver builder: {e}")),
    };

    let mx_records = match resolve_mx(&resolver, domain).await {
        Ok(v) => v,
        Err(e) => return Outcome::Transient(format!("mx lookup: {e}")),
    };
    if mx_records.is_empty() {
        return Outcome::Transient(format!("no MX for {domain}"));
    }

    // MTA-STS policy (G8): enforce mode forbids plaintext downgrade and
    // restricts delivery to the policy's mx: set. testing/none/absent =
    // opportunistic (unchanged). Fail-open on any discovery error.
    let sts_policy = mailrs_fastcore::sender_sts::fetch_policy(&cfg.kevy_url, domain).await;
    let sts_enforce = sts_policy
        .as_ref()
        .map(mailrs_fastcore::sender_sts::is_enforce)
        .unwrap_or(false);

    let timeouts = TimeoutConfig::default();
    let mut last_err = String::from("no MX host attempted");

    for mx in &mx_records {
        // enforce: skip MX not covered by the policy's mx: patterns
        if let Some(policy) = &sts_policy
            && sts_enforce
            && mailrs_fastcore::sender_sts::mx_decision(policy, &mx.exchange)
                == mailrs_mta_sts::Decision::Deny
        {
            last_err = format!("mta-sts enforce: {} not in policy mx:", mx.exchange);
            tracing::warn!(err = %last_err, "MX excluded by STS policy, next MX");
            mailrs_fastcore::tlsrpt::record(
                &cfg.kevy_url,
                &mailrs_fastcore::tlsrpt::TlsEvent {
                    domain: domain.to_string(),
                    mx: mx.exchange.to_string(),
                    success: false,
                    failure_type: Some("mx-mismatch".into()),
                    detail: Some(last_err.clone()),
                },
            );
            continue;
        }
        tracing::info!(mx = %mx.exchange, priority = mx.priority, %recipient, "attempt");
        let conn = match SmtpConnection::connect_with_timeout(&mx.exchange, 25, &timeouts).await {
            Ok(c) => c,
            Err(e) => {
                last_err = format!("connect {}: {e}", mx.exchange);
                tracing::warn!(err = %last_err, "connect failed, next MX");
                continue;
            }
        };

        // First EHLO (plain).
        let mut conn = conn;
        let mut tls_used = false;
        if let Err(e) = conn.ehlo(&cfg.helo).await {
            last_err = format!("ehlo {}: {e}", mx.exchange);
            tracing::warn!(err = %last_err, "ehlo failed, next MX");
            continue;
        }

        // Opportunistic STARTTLS with plaintext downgrade on failure.
        //
        // - Success: upgrade + re-EHLO
        // - Rejected (server refused STARTTLS): stay on the same plaintext conn
        // - HandshakeFailed (peer cert expired / SNI mismatch / etc.):
        //   the TCP session is dead, so open a fresh plaintext session
        //   and continue there. This matches how Gmail/O365/Postfix
        //   handle opportunistic-TLS failures — SPF/DKIM/DMARC (not TLS)
        //   are the real integrity guarantees for interpersonal mail.
        // DANE (RFC 7672 / G8.2): if the MX publishes DNSSEC-anchored
        // TLSA records, TLS is mandatory and the cert is verified
        // against them — a missing/failed handshake must NOT downgrade.
        let tlsa = mailrs_smtp_client::resolve_tlsa(&resolver, &mx.exchange).await;
        let dane_active = !tlsa.is_empty();
        let starttls = if dane_active {
            let cfg = mailrs_smtp_client::dane_tls_config(tlsa);
            conn.try_starttls_with_config(&mx.exchange, cfg).await
        } else {
            conn.try_starttls(&mx.exchange).await
        };
        let conn = match starttls {
            mailrs_smtp_client::StarttlsResult::Success(c) => {
                let mut c = c;
                if let Err(e) = c.ehlo(&cfg.helo).await {
                    last_err = format!("ehlo-after-starttls {}: {e}", mx.exchange);
                    tracing::warn!(err = %last_err, "post-tls ehlo failed, next MX");
                    continue;
                }
                tls_used = true;
                c
            }
            mailrs_smtp_client::StarttlsResult::Rejected {
                conn,
                code,
                message: msg,
            } => {
                if sts_enforce || dane_active {
                    last_err = format!("mta-sts enforce: {} refused STARTTLS", mx.exchange);
                    tracing::warn!(err = %last_err, "STARTTLS refused under STS enforce, next MX");
                    mailrs_fastcore::tlsrpt::record(
                        &cfg.kevy_url,
                        &mailrs_fastcore::tlsrpt::TlsEvent {
                            domain: domain.to_string(),
                            mx: mx.exchange.to_string(),
                            success: false,
                            failure_type: Some("starttls-not-supported".into()),
                            detail: Some(last_err.clone()),
                        },
                    );
                    let mut c = conn;
                    let _ = c.quit().await;
                    continue;
                }
                tracing::info!(code, %msg, "STARTTLS rejected, continuing plaintext");
                conn
            }
            mailrs_smtp_client::StarttlsResult::HandshakeFailed { source, .. } => {
                if sts_enforce || dane_active {
                    last_err = format!("mta-sts enforce: {} TLS handshake failed", mx.exchange);
                    tracing::warn!(err = %last_err, "TLS handshake failed under STS enforce, next MX");
                    continue;
                }
                tracing::warn!(
                    err = %source,
                    mx = %mx.exchange,
                    "STARTTLS handshake failed, downgrading to plaintext"
                );
                let mut plain = match SmtpConnection::connect_with_timeout(
                    &mx.exchange,
                    25,
                    &timeouts,
                )
                .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        last_err = format!("plaintext reconnect {}: {e}", mx.exchange);
                        tracing::warn!(err = %last_err, "reconnect after TLS failure failed, next MX");
                        continue;
                    }
                };
                if let Err(e) = plain.ehlo(&cfg.helo).await {
                    last_err = format!("plaintext ehlo {}: {e}", mx.exchange);
                    tracing::warn!(err = %last_err, "plaintext ehlo after TLS failure failed, next MX");
                    continue;
                }
                plain
            }
        };

        let mut conn = conn;
        let resp = match conn.deliver(sender, &[recipient], payload).await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("deliver {}: {e}", mx.exchange);
                tracing::warn!(err = %last_err, "deliver io error, next MX");
                let _ = conn.quit().await;
                continue;
            }
        };

        let _ = conn.quit().await;

        if resp.is_positive() {
            tracing::info!(mx = %mx.exchange, code = resp.code, msg = %resp.message(), "delivered");
            // TLS-RPT success event (G8.3): tls_used tracks whether the
            // final connection upgraded — set at STARTTLS resolution.
            mailrs_fastcore::tlsrpt::record(
                &cfg.kevy_url,
                &mailrs_fastcore::tlsrpt::TlsEvent {
                    domain: domain.to_string(),
                    mx: mx.exchange.to_string(),
                    success: tls_used,
                    failure_type: (!tls_used).then(|| "starttls-not-supported".to_string()),
                    detail: None,
                },
            );
            return Outcome::Delivered;
        }
        if resp.is_permanent_error() {
            let msg = format!("{} {} {}", mx.exchange, resp.code, resp.message());
            tracing::warn!(err = %msg, "permanent rejection");
            return Outcome::Permanent(msg);
        }
        // Transient (4xx): try next MX before giving up on this attempt.
        last_err = format!("{} {} {}", mx.exchange, resp.code, resp.message());
        tracing::warn!(err = %last_err, "transient rejection, next MX");
    }

    Outcome::Transient(last_err)
}

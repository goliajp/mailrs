//! Delivering one message: MX resolution, the SMTP conversation, ARC and
//! DKIM, and what to record about how it went.

use std::borrow::Cow;

use super::config::*;
use super::*;

mod attempt;

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
    let payload = match sealed_and_signed(cfg, sender, original_sender, message) {
        Ok(payload) => payload,
        Err(outcome) => return outcome,
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

    let (resolver, mx_records) = match resolve_mx_hosts(domain).await {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };

    // MTA-STS policy (G8): enforce mode forbids plaintext downgrade and
    // restricts delivery to the policy's mx: set. testing/none/absent =
    // opportunistic (unchanged). Fail-open on any discovery error.
    let sts_policy = mailrs_fastcore::sender_sts::fetch_policy(&cfg.kevy_url, domain).await;
    let sts_enforce = sts_policy
        .as_ref()
        .map(mailrs_fastcore::sender_sts::is_enforce)
        .unwrap_or(false);

    let timeouts = TimeoutConfig::default();
    let attempt = attempt::Attempt {
        cfg,
        resolver: &resolver,
        domain,
        sender,
        recipient,
        payload: &payload,
        sts_policy: sts_policy.as_ref(),
        sts_enforce,
        timeouts: &timeouts,
    };
    let mut last_err = String::from("no MX host attempted");
    for mx in &mx_records {
        if let Some(outcome) = attempt.mx(mx, &mut last_err).await {
            return outcome;
        }
    }

    Outcome::Transient(last_err)
}

/// The bytes that go on the wire: ARC-sealed when this is a forward,
/// then DKIM-signed when a key is configured.
fn sealed_and_signed<'a>(
    cfg: &Cfg,
    sender: &str,
    original_sender: &str,
    message: &'a [u8],
) -> Result<Cow<'a, [u8]>, Outcome> {
    // ARC-seal first, so the DKIM signature covers a message that
    // already carries its ARC set.
    //
    // A forward is where the envelope sender differs from the original
    // sender — the SRS rewrite in enqueue_redirect produces exactly
    // that. Ordinary sends have the two equal, and system mail has both
    // as the null sender, so neither gets sealed.
    let message: Cow<'a, [u8]> = match arc_seal_target(cfg, sender, original_sender, message) {
        Some(bytes) => Cow::Owned(bytes),
        None => Cow::Borrowed(message),
    };

    // DKIM sign if a signing key is configured. Fatal signing errors
    // are permanent (won't heal on retry): message data is malformed
    // or the private key is broken.
    match cfg.dkim.as_ref() {
        Some(dkim) => match dkim.sign(&message) {
            Ok(signed) => {
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
                Ok(Cow::Owned(signed))
            }
            Err(e) => Err(Outcome::Permanent(format!("dkim sign: {e}"))),
        },
        None => Ok(message),
    }
}

/// A resolver, and the domain's MX hosts in priority order.
async fn resolve_mx_hosts(
    domain: &str,
) -> Result<(TokioResolver, Vec<mailrs_smtp_client::MxRecord>), Outcome> {
    let resolver = match TokioResolver::builder_tokio() {
        Ok(b) => match b.build() {
            Ok(r) => r,
            Err(e) => return Err(Outcome::Transient(format!("resolver build: {e}"))),
        },
        Err(e) => return Err(Outcome::Transient(format!("resolver builder: {e}"))),
    };

    let mx_records = match resolve_mx(&resolver, domain).await {
        Ok(v) => v,
        Err(e) => return Err(Outcome::Transient(format!("mx lookup: {e}"))),
    };
    if mx_records.is_empty() {
        return Err(Outcome::Transient(format!("no MX for {domain}")));
    }
    Ok((resolver, mx_records))
}

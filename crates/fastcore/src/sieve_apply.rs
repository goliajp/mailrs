//! Sieve script application for the spool drain.
//!
//! For each spool file whose recipient resolves to a local maildir we
//! look up the recipient's `sieve:<address>` script in the network
//! kevy, populated by webapi's `PUT /api/admin/accounts/{addr}/sieve`
//! (and the ManageSieve session on port 4190 once that comes back).
//! When one is set, we compile it and evaluate against the raw message
//! bytes; the resulting actions become a delivery decision.
//!
//! Current coverage vs [`SieveAction`]:
//! - `Keep` → default deliver to INBOX (unchanged)
//! - `FileInto` → deliver to `<maildir>/.<folder>/new/`
//! - `Discard` → mark delivered without writing (silent drop)
//! - `Redirect` → enqueue outbound to the target, drop original
//! - `Reject` → RFC 3464 DSN (5.7.1) to the envelope sender via the
//!   outbound queue, message not delivered (G4.1). Backscatter guard:
//!   only when the receiver's antispam verdict routed to INBOX and the
//!   envelope sender is a real address — otherwise silently discarded
//! - `Vacation` → auto-reply with kevy dedup + RFC 3834 suppression,
//!   orthogonal to the disposition (implicit keep still applies, G4.2)

use kevy_client::Connection;
use mailrs_sieve::{SieveAction, compile_sieve, evaluate_sieve_with_envelope};

/// The delivery decision the drain applies after consulting sieve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Deliver to INBOX/{new} — the default when no sieve applies.
    Keep,
    /// Deliver into `<maildir>/.<folder>/{new}`. The folder string is
    /// verbatim from the script; the caller sanitises path separators.
    FileInto(String),
    /// Drop the file silently. `Ok(true)` back to the drain loop.
    Discard,
    /// Enqueue an outbound relay to the target address before dropping
    /// the local file.
    Redirect(String),
    /// Refuse delivery: DSN the envelope sender (when the backscatter
    /// guard allows) and drop the message. Reason is the script's text.
    Reject(String),
}

/// Full sieve verdict: the disposition plus an orthogonal vacation
/// action (RFC 5230 — vacation does not cancel the implicit keep).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SieveOutcome {
    /// What happens to the message itself.
    pub decision: Decision,
    /// Auto-reply to generate after successful local delivery.
    pub vacation: Option<Vacation>,
}

/// Parsed `vacation` parameters the drain needs to build the reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vacation {
    /// Reply body.
    pub reason: String,
    /// `:subject` override.
    pub subject: Option<String>,
    /// `:from` override.
    pub from: Option<String>,
    /// `:handle` dedup key.
    pub handle: Option<String>,
    /// Dedup window seconds (default 7 days per RFC 5230 §4.1).
    pub period_secs: u64,
    /// `:addresses` the script considers "mine".
    pub addresses: Vec<String>,
}

/// Load + compile + evaluate the sieve script for `recipient` against
/// the raw message. Returns `Decision::Keep` when there's no script,
/// the script fails to compile, or no action fires.
///
/// Runs sync — spool_drain is already sync, and network kevy calls
/// through kevy-client are blocking. Called at most a handful of
/// times per drain tick, so no need for pooling.
pub fn decide(recipient: &str, raw_message: &[u8], envelope_from: Option<&str>) -> SieveOutcome {
    let keep = SieveOutcome {
        decision: Decision::Keep,
        vacation: None,
    };
    let Some(script) = fetch_script(recipient) else {
        return keep;
    };
    let compiled = match compile_sieve(&script) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(recipient, error = %e, "sieve: compile failed; keeping default delivery");
            return keep;
        }
    };
    let actions =
        evaluate_sieve_with_envelope(&compiled, raw_message, envelope_from, Some(recipient));
    let vacation = actions.iter().find_map(|a| match a {
        SieveAction::Vacation {
            reason,
            subject,
            from,
            handle,
            period_secs,
            addresses,
            mime: _,
        } => Some(Vacation {
            reason: reason.clone(),
            subject: subject.clone(),
            from: from.clone(),
            handle: handle.clone(),
            period_secs: period_secs.unwrap_or(7 * 86_400),
            addresses: addresses.clone(),
        }),
        _ => None,
    });
    SieveOutcome {
        decision: action_to_decision(&actions),
        vacation,
    }
}

/// Reduce a script's action list to a single delivery decision. Sieve
/// scripts may emit multiple actions (e.g. `fileinto` + `keep`); we
/// honor the strongest one in this order:
/// Discard > Redirect > Reject/Vacation > FileInto > Keep.
fn action_to_decision(actions: &[SieveAction]) -> Decision {
    if actions.iter().any(|a| matches!(a, SieveAction::Discard)) {
        return Decision::Discard;
    }
    if let Some(reason) = actions.iter().find_map(|a| match a {
        SieveAction::Reject(r) => Some(r.clone()),
        _ => None,
    }) {
        return Decision::Reject(reason);
    }
    if let Some(target) = actions.iter().find_map(|a| match a {
        SieveAction::Redirect(t) => Some(t.clone()),
        _ => None,
    }) {
        return Decision::Redirect(target);
    }
    if let Some(folder) = actions.iter().find_map(|a| match a {
        SieveAction::FileInto { mailbox, .. } => Some(mailbox.clone()),
        _ => None,
    }) {
        return Decision::FileInto(folder);
    }
    Decision::Keep
}

/// RFC 5230 / RFC 3834 vacation auto-reply. Called by the drain after
/// a successful LOCAL delivery. All the "never reply" rules live here:
///
/// - envelope sender null / MAILER-DAEMON / postmaster
/// - sender == recipient (self-mail)
/// - Auto-Submitted present and != "no" (never answer another robot)
/// - Precedence: bulk / list / junk, or a List-Id header (mailing lists)
/// - the recipient (or one of `:addresses`) does not appear in To/Cc
/// - a reply for (recipient, handle, sender) already went out inside
///   the dedup window (network-kevy key with TTL)
pub fn maybe_vacation_reply(recipient: &str, envelope_from: &str, raw: &[u8], vac: &Vacation) {
    if crate::bounce::suppress_bounce(envelope_from) {
        return;
    }
    let sender = envelope_from
        .trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_lowercase();
    if sender == recipient.to_lowercase() {
        return;
    }
    if let Some(auto) = crate::bounce::header_value(raw, "Auto-Submitted")
        && !auto.eq_ignore_ascii_case("no")
    {
        return;
    }
    if let Some(prec) = crate::bounce::header_value(raw, "Precedence") {
        let p = prec.to_ascii_lowercase();
        if p.contains("bulk") || p.contains("list") || p.contains("junk") {
            return;
        }
    }
    if crate::bounce::header_value(raw, "List-Id").is_some() {
        return;
    }
    // addressed-to-me check (RFC 5230 §4.4)
    let to_cc = format!(
        "{} {}",
        crate::bounce::header_value(raw, "To").unwrap_or_default(),
        crate::bounce::header_value(raw, "Cc").unwrap_or_default()
    )
    .to_lowercase();
    let mut mine = vec![recipient.to_lowercase()];
    mine.extend(vac.addresses.iter().map(|a| a.to_lowercase()));
    if !mine.iter().any(|a| to_cc.contains(a.as_str())) {
        tracing::debug!(
            recipient,
            "vacation: message not addressed to me — no reply"
        );
        return;
    }
    // dedup window
    let Ok(url) = std::env::var("MAILRS_KEVY_URL") else {
        return;
    };
    let Ok(mut conn) = Connection::open(&url) else {
        return;
    };
    let handle = vac.handle.clone().unwrap_or_else(|| "default".into());
    let dedup_key = format!("mailrs:vacation:{recipient}:{handle}:{sender}");
    if conn.get(dedup_key.as_bytes()).ok().flatten().is_some() {
        return;
    }
    let _ = conn.set(dedup_key.as_bytes(), b"1");
    let _ = conn.expire(
        dedup_key.as_bytes(),
        std::time::Duration::from_secs(vac.period_secs),
    );

    // compose the reply
    let orig_subject = crate::bounce::header_value(raw, "Subject").unwrap_or_default();
    let orig_mid = crate::bounce::header_value(raw, "Message-ID")
        .map(|m| m.trim_matches(|c| c == '<' || c == '>').to_string());
    let subject = vac
        .subject
        .clone()
        .unwrap_or_else(|| format!("Auto: {orig_subject}"));
    let from = vac.from.clone().unwrap_or_else(|| recipient.to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let date = chrono::DateTime::from_timestamp(now as i64, 0)
        .map(|d| d.to_rfc2822())
        .unwrap_or_default();
    let mut msg = String::new();
    msg.push_str(&format!("From: <{from}>\r\n"));
    msg.push_str(&format!("To: <{sender}>\r\n"));
    msg.push_str(&format!("Subject: {subject}\r\n"));
    msg.push_str(&format!("Date: {date}\r\n"));
    msg.push_str(&format!(
        "Message-ID: <vacation-{now}@{from_domain}>\r\n",
        from_domain = from.split('@').nth(1).unwrap_or("mailrs")
    ));
    if let Some(mid) = &orig_mid {
        msg.push_str(&format!("In-Reply-To: <{mid}>\r\nReferences: <{mid}>\r\n"));
    }
    msg.push_str("Auto-Submitted: auto-replied\r\nPrecedence: bulk\r\n");
    msg.push_str("Content-Type: text/plain; charset=utf-8\r\nMIME-Version: 1.0\r\n\r\n");
    msg.push_str(&vac.reason);
    msg.push_str("\r\n");

    match enqueue_vacation_outbound(&mut conn, &sender, msg.as_bytes()) {
        Ok(()) => tracing::info!(recipient, %sender, "vacation: auto-reply queued"),
        Err(e) => tracing::warn!(recipient, error = %e, "vacation: enqueue failed"),
    }
}

/// Push the composed vacation reply onto the outbound queue with a
/// null envelope sender (RFC 3834 §3.3 — auto-replies must not bounce).
fn enqueue_vacation_outbound(conn: &mut Connection, to: &str, body: &[u8]) -> std::io::Result<()> {
    use base64::Engine as _;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let id = format!("{now_ms}-vac");
    let b64 = base64::engine::general_purpose::STANDARD.encode(body);
    let envelope = serde_json::json!({
        "sender": "<>",
        "recipient": to,
        "message_data_b64": b64,
        "attempts": 0,
        "next_attempt": 0,
        "id": &id,
        "envelope_from": "<>",
    });
    let key = format!("mailrs:outbound:{id}");
    conn.hset(
        key.as_bytes(),
        &[(b"blob".as_slice(), envelope.to_string().as_bytes())],
    )
    .map_err(std::io::Error::other)?;
    conn.lpush(b"mailrs:outbound:pending", &[id.as_bytes()])
        .map_err(std::io::Error::other)?;
    Ok(())
}

/// Read `sieve:<address>` from the network kevy. Empty / unset → None.
fn fetch_script(address: &str) -> Option<String> {
    let url = std::env::var("MAILRS_KEVY_URL").ok()?;
    let mut conn = Connection::open(&url).ok()?;
    let key = format!("sieve:{address}");
    let raw = conn.get(key.as_bytes()).ok().flatten()?;
    let script = String::from_utf8(raw).ok()?;
    if script.trim().is_empty() {
        return None;
    }
    Some(script)
}

/// Turn a Maildir++ folder name from `fileinto` into a safe
/// disk-path segment. Dot-prefixed, and any embedded `/` is replaced
/// with `.` per the Maildir++ hierarchy convention.
pub fn maildir_subfolder(folder: &str) -> String {
    let cleaned = folder.trim().replace('/', ".").replace(['\\', '\0'], "");
    if cleaned.starts_with('.') {
        cleaned
    } else {
        format!(".{cleaned}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_when_no_actions() {
        assert_eq!(action_to_decision(&[]), Decision::Keep);
    }

    #[test]
    fn discard_beats_all() {
        let a = [
            SieveAction::FileInto {
                mailbox: "Spam".into(),
                flags: vec![],
            },
            SieveAction::Discard,
        ];
        assert_eq!(action_to_decision(&a), Decision::Discard);
    }

    #[test]
    fn redirect_returns_redirect() {
        let a = [SieveAction::Redirect("b@y".into())];
        assert_eq!(action_to_decision(&a), Decision::Redirect("b@y".into()));
    }

    #[test]
    fn fileinto_returns_folder() {
        let a = [SieveAction::FileInto {
            mailbox: "Work".into(),
            flags: vec![],
        }];
        assert_eq!(action_to_decision(&a), Decision::FileInto("Work".into()));
    }

    #[test]
    fn reject_is_a_first_class_decision() {
        let a = vec![SieveAction::Reject("no thanks".into())];
        assert_eq!(action_to_decision(&a), Decision::Reject("no thanks".into()));
    }

    #[test]
    fn vacation_falls_back_to_keep() {
        let a = [SieveAction::Vacation {
            reason: "OOO".into(),
            subject: None,
            from: None,
            handle: None,
            period_secs: None,
            addresses: vec![],
            mime: false,
        }];
        assert_eq!(action_to_decision(&a), Decision::Keep);
    }

    #[test]
    fn subfolder_maps_slashes_to_dots() {
        assert_eq!(maildir_subfolder("Work/Client"), ".Work.Client");
    }

    #[test]
    fn subfolder_preserves_leading_dot() {
        assert_eq!(maildir_subfolder(".Junk"), ".Junk");
    }

    #[test]
    fn subfolder_strips_backslash_and_nul() {
        assert_eq!(maildir_subfolder("Work\\x\0y"), ".Workxy");
    }
}

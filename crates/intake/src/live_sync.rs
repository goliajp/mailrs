//! Live post-arrival sink — the contacts autocomplete store.
//!
//! `mailrs:user:<u>:contacts` in the **network** kevy powers
//! `/api/contacts` autocomplete. It was previously populated only by
//! the one-shot `mailrs-fastcore-backfill-contacts` binary, so live
//! arrivals past the backfill were invisible to it.
//!
//! Best-effort — an unreachable kevy-server never blocks or crashes the
//! caller.
//!
//! Full-text search used to have a sink here too, writing into a
//! Meilisearch sidecar. That was removed once the thread rows grew a
//! kevy text index: a second copy of the index in another process could
//! drift from the rows, and did (2026-07-19).

use kevy_client::Connection;

/// Parse a comma-separated senders_csv into `(email, display)` tuples.
/// Handles `Name <email>`, bare `email`, and mixed cases.
pub fn parse_senders(csv: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for token in csv.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if let Some(lt) = token.rfind('<')
            && let Some(gt) = token.rfind('>')
            && gt > lt
        {
            let email = token[lt + 1..gt].trim().to_string();
            if email.contains('@') {
                out.push((email, token.to_string()));
                continue;
            }
        }
        if token.contains('@') {
            out.push((token.to_string(), token.to_string()));
        }
    }
    out
}

/// Push each parsed (email, display) into `mailrs:user:<user>:contacts`
/// in the network kevy. Best-effort; unreachable kevy just skips.
pub fn upsert_contacts(user: &str, senders_csv: &str) {
    // Write-side guard (2026-07-18): decode RFC 2047 encoded-words here
    // so a caller feeding a pre-decode-era senders_csv (old thread rows,
    // backfills) can't poison the contact store with `=?…?=` display
    // names again. Idempotent on already-decoded input.
    let decoded = mailrs_rfc2047::decode(senders_csv.as_bytes()).into_owned();
    let parsed = parse_senders(&decoded);
    if parsed.is_empty() {
        return;
    }
    let Some(url) = network_kevy_url() else {
        return;
    };
    let Ok(mut conn) = Connection::connect(&url) else {
        return;
    };
    let key = mailrs_core_sidestate::families::contact_cards::autocomplete_key(user);
    let pairs: Vec<(Vec<u8>, Vec<u8>)> = parsed
        .into_iter()
        .map(|(email, display)| (email.into_bytes(), display.into_bytes()))
        .collect();
    let refs: Vec<(&[u8], &[u8])> = pairs
        .iter()
        .map(|(k, v)| (k.as_slice(), v.as_slice()))
        .collect();
    let _ = conn.hset(key.as_bytes(), &refs);
    // How familiar this sender's domain is, which is half of the
    // brand-impersonation check — a phish claiming to be Amazon comes
    // from a domain nothing has ever arrived from, and a newsletter
    // that merely names Amazon comes from one with a history. Written
    // here because this is the one place every delivered message
    // already passes through with the sender in hand.
    for (email, _) in parse_senders(&decoded) {
        if let Some(host) = email.rsplit('@').next() {
            mailrs_core_sidestate::families::domain_history::record_seen(&mut conn, host);
        }
    }
}

/// Append a system-event audit fact to the shared `admin:audit_log`
/// stream (G12.3) — the same list webapi's admin API and audit UI
/// read. fastcore's system events (bounce delivered, sender permanent
/// failure, quota reject) belong in the same operator-visible trail as
/// UI-driven admin actions. Best-effort; actor is always "system".
pub fn audit_system(action: &str, target: &str, detail: &str) {
    let Some(url) = network_kevy_url() else {
        return;
    };
    let Ok(mut conn) = Connection::connect(&url) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let id = conn
        .incr(mailrs_core_sidestate::keys::admin::AUDIT_LOG_COUNTER)
        .unwrap_or(0);
    let fact = serde_json::json!({
        "id": id,
        "occurred_at": now,
        "recorded_at": now,
        "actor": "system",
        "action": action,
        "target": target,
        "detail": detail,
    });
    if let Ok(bytes) = serde_json::to_vec(&fact) {
        let _ = conn.lpush(
            mailrs_core_sidestate::keys::admin::AUDIT_LOG,
            &[bytes.as_slice()],
        );
    }
}

pub fn network_kevy_url() -> Option<String> {
    std::env::var("MAILRS_KEVY_URL").ok()
}

/// Publish the frontend-shaped realtime event AFTER the message is
/// readable in kevy. The receiver's SpoolDelivered fires at spool-write
/// time, before the drain ingests, so a client refetching on it would
/// find nothing; it only wakes the drain. webapi reads these keys off the
/// change feed and sends each to its user's sockets as they are; this
/// payload IS the frontend contract (web/src/lib/types.ts NewMessageEvent).
///
/// `held`: the conversation is held for review. The clients still refresh
/// what they show, so Review stays live, but raise no notice for it.
pub fn publish_new_mail(
    user: &str,
    thread_id: &str,
    sender: &str,
    subject: &str,
    snippet: &str,
    held: bool,
) {
    let Some(url) = network_kevy_url() else {
        return;
    };
    let Ok(mut conn) = Connection::connect(&url) else {
        return;
    };
    let payload = serde_json::json!({
        "type": "NewMessage",
        "user": user,
        "thread_id": thread_id,
        "sender": sender,
        "subject": subject,
        "snippet": snippet,
        "held": held,
    });
    let Ok(bytes) = serde_json::to_vec(&payload) else {
        return;
    };
    let _ = mailrs_core_sidestate::keys::events::send(&mut conn, &bytes);
}

/// Adjust the user's used-bytes counter in `net`, the network kevy the
/// calling core names — the receiver's quota stage reads
/// `mailrs:quota:<user>:used_bytes` at RCPT time; `None` counts nothing.
/// Best-effort: kevy down means the counter drifts until the next
/// backfill-usage run; enforcement is fail-open anyway.
pub fn adjust_usage_on(net: Option<&mut Connection>, user: &str, delta: i64) {
    let Some(conn) = net else {
        return;
    };
    if delta == 0 {
        return;
    }
    let key = mailrs_core_sidestate::keys::quota::used_key(user);
    let _ = conn.incr_by(key.as_bytes(), delta);
}

/// Read `(limit, used)` bytes for a user from the network kevy the
/// calling core names. `(0, _)` = no limit configured. Fail-open: any
/// error reads as 0.
pub fn quota_read_on(conn: &mut Connection, user: &str) -> (i64, i64) {
    let lk = mailrs_core_sidestate::keys::quota::limit_key(user);
    let uk = mailrs_core_sidestate::keys::quota::used_key(user);
    let parse = |v: Option<Vec<u8>>| {
        v.and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    };
    let limit = parse(conn.get(lk.as_bytes()).unwrap_or(None));
    let used = parse(conn.get(uk.as_bytes()).unwrap_or(None));
    (limit, used)
}

/// The quota check on write paths the core owns (IMAP APPEND), over the
/// network kevy the calling core names. Missing/zero limit = unlimited;
/// no store (`None`) or any error reads as not exceeded (fail-open).
pub fn quota_exceeded_on(net: Option<&mut Connection>, user: &str) -> bool {
    let Some(conn) = net else {
        return false;
    };
    let (limit, used) = quota_read_on(conn, user);
    limit > 0 && used >= limit
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn parses_display_email_pair() {
        let out = parse_senders("Alice <a@x.com>, Bob <b@y.com>");
        assert_eq!(
            out,
            vec![
                ("a@x.com".to_string(), "Alice <a@x.com>".to_string()),
                ("b@y.com".to_string(), "Bob <b@y.com>".to_string()),
            ]
        );
    }

    #[test]
    fn parses_bare_emails() {
        let out = parse_senders("a@x.com, b@y.com");
        assert_eq!(
            out,
            vec![
                ("a@x.com".to_string(), "a@x.com".to_string()),
                ("b@y.com".to_string(), "b@y.com".to_string()),
            ]
        );
    }

    #[test]
    fn skips_non_email_tokens() {
        let out = parse_senders("Alice, no-at-sign, <malformed");
        assert!(out.is_empty());
    }
}

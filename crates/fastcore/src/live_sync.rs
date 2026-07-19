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
    let Ok(mut conn) = Connection::open(&url) else {
        return;
    };
    let key = format!("mailrs:user:{user}:contacts");
    let pairs: Vec<(Vec<u8>, Vec<u8>)> = parsed
        .into_iter()
        .map(|(email, display)| (email.into_bytes(), display.into_bytes()))
        .collect();
    let refs: Vec<(&[u8], &[u8])> = pairs
        .iter()
        .map(|(k, v)| (k.as_slice(), v.as_slice()))
        .collect();
    let _ = conn.hset(key.as_bytes(), &refs);
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
    let Ok(mut conn) = Connection::open(&url) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let id = conn.incr(b"admin:audit_log:counter").unwrap_or(0);
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
        let _ = conn.lpush(b"admin:audit_log", &[bytes.as_slice()]);
    }
}

pub(crate) fn network_kevy_url() -> Option<String> {
    std::env::var("MAILRS_KEVY_URL").ok()
}

/// Publish the frontend-shaped realtime event AFTER the message is
/// readable in kevy. The receiver's SpoolDelivered fires at spool-write
/// time — before the drain ingests — so a webapp refetch triggered by
/// it finds nothing; worse, its NotifyEnvelope wrapper isn't the
/// `{type:"NewMessage",...}` shape the webapp matches on, so realtime
/// sync was dead in the fastcore topology. webapi forwards whatever
/// crosses `notify:new-mail` verbatim; this payload IS the frontend
/// contract (web/src/lib/types.ts NewMessageEvent).
pub fn publish_new_mail(user: &str, thread_id: &str, sender: &str, subject: &str, snippet: &str) {
    let Some(url) = network_kevy_url() else {
        return;
    };
    let Ok(mut conn) = Connection::open(&url) else {
        return;
    };
    let payload = serde_json::json!({
        "type": "NewMessage",
        "user": user,
        "thread_id": thread_id,
        "sender": sender,
        "subject": subject,
        "snippet": snippet,
    });
    let Ok(bytes) = serde_json::to_vec(&payload) else {
        return;
    };
    // v2.3 §P7-C (2026-07-12): legacy pubsub PUBLISH dropped. The
    // feed_read consumer wired in webapi handles realtime delivery
    // via kevy's change feed — durable across webapi restarts
    // (unlike PUBSUB which discards messages when no subscriber is
    // attached at publish time). SET+EXPIRE 300 keeps the AOF bound
    // — a consumer offline > 5 min misses those events, which is
    // fine because the frontend refetches full state on WS reconnect.
    let key = format!(
        "mailrs:events:notify:{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let _ = conn.pipeline(|p| {
        p.cmd(&[b"SET", key.as_bytes(), bytes.as_slice()]);
        p.cmd(&[b"EXPIRE", key.as_bytes(), b"300"]);
    });
}

/// Adjust the recipient's used-bytes counter in the NETWORK kevy —
/// the receiver's quota stage reads `mailrs:quota:<user>:used_bytes`
/// at RCPT time. Best-effort: kevy down means the counter drifts until
/// the next backfill-usage run; enforcement is fail-open anyway.
pub fn adjust_usage_bytes(user: &str, delta: i64) {
    if delta == 0 {
        return;
    }
    let Some(url) = network_kevy_url() else {
        return;
    };
    let Ok(mut conn) = Connection::open(&url) else {
        return;
    };
    let key = format!("mailrs:quota:{}:used_bytes", user.to_lowercase());
    let _ = conn.incr_by(key.as_bytes(), delta);
}

/// Mirror the quota LIMIT to the network kevy so the receiver's quota
/// stage can consult it (the authoritative copy stays in the account
/// blob in fastcore's embedded store).
pub fn mirror_quota_limit(user: &str, limit_bytes: i64) {
    let Some(url) = network_kevy_url() else {
        return;
    };
    let Ok(mut conn) = Connection::open(&url) else {
        return;
    };
    let key = format!("mailrs:quota:{}:limit_bytes", user.to_lowercase());
    let _ = conn.set(key.as_bytes(), limit_bytes.to_string().as_bytes());
}

/// Read `(limit, used)` bytes for a user from the network kevy.
/// `(0, _)` = no limit configured. Fail-open: any error reads as 0.
pub fn quota_read(user: &str) -> (i64, i64) {
    let Some(url) = network_kevy_url() else {
        return (0, 0);
    };
    let Ok(mut conn) = Connection::open(&url) else {
        return (0, 0);
    };
    let lk = format!("mailrs:quota:{}:limit_bytes", user.to_lowercase());
    let uk = format!("mailrs:quota:{}:used_bytes", user.to_lowercase());
    let parse = |v: Option<Vec<u8>>| {
        v.and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    };
    let limit = parse(conn.get(lk.as_bytes()).unwrap_or(None));
    let used = parse(conn.get(uk.as_bytes()).unwrap_or(None));
    (limit, used)
}

/// Read (limit, used) for the quota check on write paths fastcore owns
/// (IMAP APPEND). Missing/zero limit = unlimited. Fail-open on errors.
pub fn quota_exceeded(user: &str) -> bool {
    let (limit, used) = quota_read(user);
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

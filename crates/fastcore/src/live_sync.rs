//! Live post-arrival sinks — contacts + Meili.
//!
//! Fastcore's self-heal creates threads in its embedded kevy. Two side
//! channels the web UI depends on need those writes too:
//!
//! - `mailrs:user:<u>:contacts` in the **network** kevy — powers
//!   `/api/contacts` autocomplete. Was previously populated only by the
//!   one-shot `mailrs-fastcore-backfill-contacts` binary; live arrivals
//!   past the backfill were invisible to the autocomplete.
//! - Meilisearch index `mailrs_<user_at_domain>` — powers
//!   `/api/conversations/search`. Since fastcore never wrote to Meili,
//!   the index froze at the last monolith moment. Webapi already fell
//!   back to a 20 000-row linear scan, but with a live indexing path
//!   Meili can go back to being the fast source of truth.
//!
//! Both connections are best-effort — missing / unreachable Meili or
//! kevy-server never blocks or crashes the caller.

use std::sync::OnceLock;

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
    let parsed = parse_senders(senders_csv);
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

/// Push one thread aggregate to Meili as a searchable document.
/// Skipped when `MAILRS_MEILI_URL` is unset. Best-effort HTTP POST; a
/// failed request is a debug log.
pub fn index_meili(
    user: &str,
    thread_id: &str,
    subject: &str,
    senders_csv: &str,
    snippet: &str,
    latest_date: i64,
) {
    let Some(base) = std::env::var("MAILRS_MEILI_URL").ok() else {
        return;
    };
    let index = meili_index(user);
    // Meili primary keys must match ^[a-zA-Z0-9_-]{1,511}$; real
    // thread_ids carry @ . = / which Meili rejects — so every document
    // POSTed with thread_id-as-key was silently 400'd and NOTHING got
    // indexed. Use a sanitized `id` as the key + keep thread_id for
    // retrieval. primaryKey=id is set explicitly on the URL.
    let url = format!("{base}/indexes/{index}/documents?primaryKey=id");
    let doc = serde_json::json!([{
        "id": meili_doc_id(thread_id),
        "thread_id": thread_id,
        "subject": subject,
        "participants": senders_csv,
        "snippet": snippet,
        "last_date": latest_date,
    }]);
    let body = doc.to_string();
    // Fire-and-forget on a blocking pool thread to avoid pulling reqwest
    // into the caller's Send bounds. Meili indexing is not on the hot
    // read path.
    std::thread::spawn(move || {
        let client = ureq_client();
        let _ = client
            .post(&url)
            .set("content-type", "application/json")
            .set("Authorization", &meili_auth_header())
            .send_string(&body);
    });
}

/// Sanitize a thread_id into a Meili-legal document id: keep
/// `[a-zA-Z0-9_-]`, map everything else to `_`, cap at 511 chars. The
/// mapping is deterministic so re-indexing the same thread upserts.
pub fn meili_doc_id(thread_id: &str) -> String {
    let mut out: String = thread_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.len() > 511 {
        out.truncate(511);
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// Meili index uid for a user. Index uids share the doc-id charset
/// constraint (`^[a-zA-Z0-9_-]+$`), so `lihao@golia.jp` must sanitize
/// to `mailrs_lihao_at_golia_jp` — the `.` in the domain is illegal
/// and silently 400'd the whole index otherwise.
pub fn meili_index(user: &str) -> String {
    let mut out = String::from("mailrs_");
    for c in user.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else if c == '@' {
            out.push_str("_at_");
        } else {
            out.push('_');
        }
    }
    out
}

/// Remove a thread's document from the user's Meili index (delete_thread).
pub fn delete_meili(user: &str, thread_id: &str) {
    let Some(base) = std::env::var("MAILRS_MEILI_URL").ok() else {
        return;
    };
    let index = meili_index(user);
    let doc_id = meili_doc_id(thread_id);
    let url = format!("{base}/indexes/{index}/documents/{doc_id}");
    std::thread::spawn(move || {
        let client = ureq_client();
        let _ = client
            .delete(&url)
            .set("Authorization", &meili_auth_header())
            .call();
    });
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

fn meili_auth_header() -> String {
    // MAILRS_MEILI_KEY is the compose/service convention (webapi reads
    // the same); MAILRS_MEILI_MASTER_KEY is the fallback the one-shot
    // backfill bin passes explicitly. Empty = unauthenticated meili.
    let key = std::env::var("MAILRS_MEILI_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .or_else(|| {
            std::env::var("MAILRS_MEILI_MASTER_KEY")
                .ok()
                .filter(|k| !k.is_empty())
        });
    match key {
        Some(k) => format!("Bearer {k}"),
        None => String::new(),
    }
}

fn ureq_client() -> &'static ureq::Agent {
    static CLIENT: OnceLock<ureq::Agent> = OnceLock::new();
    CLIENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(5))
            .build()
    })
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
    let _ = conn.publish(b"notify:new-mail", &bytes);
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
    use super::meili_doc_id;

    #[test]
    fn meili_index_sanitizes_domain_dot() {
        assert_eq!(
            super::meili_index("lihao@golia.jp"),
            "mailrs_lihao_at_golia_jp"
        );
        assert_eq!(super::meili_index("a-b_c@x.y.z"), "mailrs_a-b_c_at_x_y_z");
    }

    #[test]
    fn meili_doc_id_sanitizes_illegal_chars() {
        assert_eq!(
            meili_doc_id("926f778fbea65115@golia.jp"),
            "926f778fbea65115_golia_jp"
        );
        assert_eq!(meili_doc_id("a/b=c"), "a_b_c");
        // already-legal ids pass through
        assert_eq!(meili_doc_id("plain-id_123"), "plain-id_123");
        // empty never yields an empty (illegal) key
        assert_eq!(meili_doc_id(""), "_");
    }

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

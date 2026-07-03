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
    let index = format!("mailrs_{}", user.replace('@', "_at_"));
    let url = format!("{base}/indexes/{index}/documents");
    let doc = serde_json::json!([{
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

fn network_kevy_url() -> Option<String> {
    std::env::var("MAILRS_KEVY_URL").ok()
}

fn meili_auth_header() -> String {
    match std::env::var("MAILRS_MEILI_MASTER_KEY") {
        Ok(k) if !k.is_empty() => format!("Bearer {k}"),
        _ => String::new(),
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

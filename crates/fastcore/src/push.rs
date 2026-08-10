//! Pushing "you have mail" to registered devices.
//!
//! A side effect of ingest, never a filter — the shape every other hook
//! in the drain follows. The whole feature is env-gated: without
//! `MAILRS_APNS_*` configured, [`maybe_notify`] is a no-op, so the
//! binary runs identically until GOLIA's APNs key exists.
//!
//! Tokens live in the network kevy under `push:tokens:{user}`, written
//! by webapi's `POST /api/push/tokens` and pruned here when Apple
//! answers `Unregistered` — Apple throttles providers that keep pushing
//! to tokens it has already refused, so pruning is part of sending
//! correctly rather than hygiene.

use std::sync::{Arc, OnceLock};

use mailrs_apns::{ApnsClient, Outcome};

/// `Some` when every `MAILRS_APNS_*` variable is present and the key
/// parses; `None` disables the feature. Resolved once — a process does
/// not gain a push key mid-flight.
fn client() -> Option<Arc<ApnsClient>> {
    static CLIENT: OnceLock<Option<Arc<ApnsClient>>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            let key_path = std::env::var("MAILRS_APNS_KEY_PATH").ok()?;
            let key_id = std::env::var("MAILRS_APNS_KEY_ID").ok()?;
            let team_id = std::env::var("MAILRS_APNS_TEAM_ID").ok()?;
            let topic = std::env::var("MAILRS_APNS_TOPIC").ok()?;
            // Sandbox unless told otherwise: development builds are the
            // first thing anyone tests, and a sandbox token pushed at
            // production reads as every device unregistering at once.
            let endpoint = std::env::var("MAILRS_APNS_ENDPOINT")
                .unwrap_or_else(|_| mailrs_apns::SANDBOX_ENDPOINT.to_string());
            let pem = match std::fs::read_to_string(&key_path) {
                Ok(pem) => pem,
                Err(e) => {
                    tracing::warn!(error = %e, %key_path, "apns: key file unreadable — push disabled");
                    return None;
                }
            };
            match ApnsClient::new(&pem, key_id, team_id, topic, endpoint) {
                Ok(c) => {
                    tracing::info!("apns: configured — push notifications enabled");
                    Some(Arc::new(c))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "apns: key rejected — push disabled");
                    None
                }
            }
        })
        .clone()
}

/// Resolve the key at boot rather than at the first delivery.
///
/// `client()` is a `OnceLock`, so without this the key is first read
/// when a message arrives — a typo in the path or a rejected key would
/// announce itself in the middle of the night, in a log line nobody is
/// reading, on the one delivery that needed it. Called from `boot`; it
/// costs one file read and answers "is push on" at the moment someone
/// is looking.
pub(crate) fn warm() {
    if client().is_none() {
        tracing::info!("apns: not configured — push disabled");
    }
}

/// Announce one delivered message to the recipient's devices.
///
/// `is_own` and the spam category are excluded here rather than at the
/// call site so the rule cannot be forgotten by a second caller: your
/// own sent copy is not news, and pushing junk would make the junk
/// filter pointless.
pub(crate) fn maybe_notify(addr: &str, sender: &str, subject: &str, category: &str, is_own: bool) {
    if is_own || category == "spam" || category == "scam" {
        return;
    }
    let Some(apns) = client() else { return };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        // The drain always runs inside the runtime; if that ever stops
        // being true, a missed push is the right failure mode.
        return;
    };
    let user = addr.to_string();
    // Header values arrive attacker-sized; the APNs payload cap is 4KB.
    let title = truncated(sender, 200);
    let body = truncated(subject, 300);
    handle.spawn(async move {
        let tokens = match tokio::task::spawn_blocking({
            let user = user.clone();
            move || load_tokens(&user)
        })
        .await
        {
            Ok(Ok(tokens)) => tokens,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, %user, "apns: token read failed");
                return;
            }
            Err(e) => {
                tracing::warn!(error = %e, "apns: token read join failed");
                return;
            }
        };
        for token in tokens {
            match apns.send_alert(&token, &title, &body).await {
                Ok(Outcome::Sent) => {}
                Ok(Outcome::Unregistered) => {
                    tracing::info!(%user, "apns: pruning dead token");
                    let user = user.clone();
                    let _ = tokio::task::spawn_blocking(move || prune_token(&user, &token)).await;
                }
                Ok(Outcome::Failed { status, reason }) => {
                    tracing::warn!(%user, status, %reason, "apns: push refused");
                }
                Err(e) => tracing::warn!(%user, error = %e, "apns: push failed"),
            }
        }
    });
}

/// The same key webapi's registration handler writes.
fn tokens_key(user: &str) -> String {
    format!("push:tokens:{user}")
}

fn load_tokens(user: &str) -> std::io::Result<Vec<String>> {
    let url = std::env::var("MAILRS_KEVY_URL")
        .map_err(|_| std::io::Error::other("MAILRS_KEVY_URL unset"))?;
    let mut conn = kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
    let pairs = conn
        .hgetall(tokens_key(user).as_bytes())
        .map_err(std::io::Error::other)?;
    // hgetall answers [field, value, field, value, …]; the fields are the
    // tokens and the values metadata this side does not read.
    Ok(pairs
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .filter_map(|(_, f)| String::from_utf8(f).ok())
        .collect())
}

fn prune_token(user: &str, token: &str) {
    let Ok(url) = std::env::var("MAILRS_KEVY_URL") else {
        return;
    };
    let Ok(mut conn) = kevy_client::Connection::connect(&url) else {
        return;
    };
    let _ = conn.hdel(tokens_key(user).as_bytes(), &[token.as_bytes()]);
}

fn truncated(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same string as `crates/webapi/src/handlers/push.rs::tokens_key`.
    /// The two lanes meet only at this key; a drift here is tokens
    /// written to one hash and read from another, which fails silently
    /// as "no devices registered".
    #[test]
    fn key_matches_the_writer() {
        assert_eq!(tokens_key("a@golia.jp"), "push:tokens:a@golia.jp");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        let cjk = "件名".repeat(300);
        let cut = truncated(&cjk, 10);
        assert_eq!(cut.chars().count(), 10);
    }
}

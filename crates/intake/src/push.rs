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
pub fn warm() {
    if client().is_none() {
        tracing::info!("apns: not configured — push disabled");
    }
    if crate::web_push::vapid().is_none() {
        tracing::info!("web push: not configured — disabled");
    }
}

/// Announce one delivered message to the recipient's devices, if the
/// reader's push policy (`crate::push_policy`) wants it, and count the
/// decision.
///
/// `is_own` and the spam category are excluded here rather than at the
/// call site so the rule cannot be forgotten by a second caller: your
/// own sent copy is not news, and pushing junk would make the junk
/// filter pointless. Nothing is counted while no push channel is
/// configured: a decision nothing could carry out is not one.
pub fn maybe_notify(
    net: Option<&mut kevy_client::Connection>,
    addr: &str,
    a: &crate::arrival::Arrival<'_>,
) {
    let (sender, subject, category) = (a.from.as_str(), a.subject.as_str(), a.category);
    if a.is_own || category == "spam" || category == "scam" {
        tracing::debug!(%addr, %category, is_own = a.is_own, "push: not news, no push");
        return;
    }
    let apns = client();
    let web = crate::web_push::vapid();
    let channels: Vec<&str> = [("apns", apns.is_some()), ("web", web.is_some())]
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(c, _)| c)
        .collect();
    if channels.is_empty() {
        tracing::debug!("push: no channel configured, no push");
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let Some(conn) = net else {
        tracing::warn!(%addr, "push: no network kevy, the push policy cannot be read; no push");
        return;
    };
    let decision =
        crate::push_policy::should_notify(conn, addr, sender, &a.thread_id, a.facts, category, now);
    if decision.verdict != crate::push_policy::Verdict::Push {
        for channel in &channels {
            let _ = mailrs_core_sidestate::families::notify::count(
                conn,
                addr,
                channel,
                decision.verdict.outcome(),
                now,
            );
        }
        tracing::debug!(%addr, ?decision, "push: held back by the push policy");
        return;
    }
    if let Some(vapid) = web {
        let payload =
            crate::web_push::payload(&a.thread_id, sender, subject, decision.hide_content);
        crate::web_push::spawn(vapid, addr, payload, now);
    }
    let Some(apns) = apns else {
        return;
    };
    // Header values arrive attacker-sized; the APNs payload cap is 4KB.
    // With the content hidden a lock screen shows only that mail came.
    let (title, body) = match decision.hide_content {
        true => ("New mail".to_string(), String::new()),
        false => (truncated(sender, 200), truncated(subject, 300)),
    };
    spawn_apns(apns, addr, title, body, now);
}

/// Tell `addr`'s devices that a conversation they had was taken back into
/// Review, under the reader's push settings: nothing when pushes are off
/// or it is their quiet time, only "moved to Review" when they hide what
/// mail says.
pub fn notify_recalled(
    net: Option<&mut kevy_client::Connection>,
    addr: &str,
    thread_id: &str,
    sender: &str,
    subject: &str,
) {
    let apns = client();
    let web = crate::web_push::vapid();
    if apns.is_none() && web.is_none() {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let Some(conn) = net else {
        tracing::warn!(%addr, "push: no network kevy, the push settings cannot be read; no push");
        return;
    };
    let prefs = mailrs_core_sidestate::families::notify::prefs(conn, addr).unwrap_or_default();
    if prefs.mode == mailrs_core_sidestate::families::notify::Mode::Off || prefs.quiet_at(now) {
        tracing::debug!(%addr, "push: a recall held back by the reader's settings");
        return;
    }
    if let Some(vapid) = web {
        let payload = crate::web_push::payload(thread_id, sender, subject, prefs.hide_content);
        crate::web_push::spawn(vapid, addr, payload, now);
    }
    let Some(apns) = apns else {
        return;
    };
    let (title, body) = match prefs.hide_content {
        true => ("Moved to Review".to_string(), String::new()),
        false => ("Moved to Review".to_string(), truncated(subject, 300)),
    };
    spawn_apns(apns, addr, title, body, now);
}

/// Send one alert to every device `addr` registered, counting each.
fn spawn_apns(apns: Arc<ApnsClient>, addr: &str, title: String, body: String, now: i64) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!(%addr, "apns: no runtime handle, no push");
        return;
    };
    let user = addr.to_string();
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
        if tokens.is_empty() {
            // Not an error — most accounts have no phone. Said out
            // loud because the alternative is silence, and silence is
            // also what a working push looks like from the log.
            tracing::info!(%user, "apns: nothing registered for this account");
        }
        for token in tokens {
            let outcome = apns.send_alert(&token, &title, &body).await;
            let counted = match &outcome {
                Ok(Outcome::Sent) => "sent",
                _ => "failed",
            };
            let who = user.clone();
            let _ = tokio::task::spawn_blocking(move || count(&who, "apns", counted, now)).await;
            match outcome {
                Ok(Outcome::Sent) => {
                    // The success path used to log nothing, so a push
                    // that worked and a push that never happened read
                    // identically — and the only way to tell them
                    // apart was to hold the phone.
                    tracing::info!(%user, "apns: delivered to Apple");
                }
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

use mailrs_core_sidestate::keys::push::tokens_key;

fn load_tokens(user: &str) -> std::io::Result<Vec<String>> {
    let url = std::env::var("MAILRS_KEVY_URL")
        .map_err(|_| std::io::Error::other("MAILRS_KEVY_URL unset"))?;
    let mut conn = kevy_client::Connection::connect(&url)?;
    let pairs = conn.hgetall(tokens_key(user).as_bytes())?;
    // hgetall answers [field, value, field, value, …]; the fields are the
    // tokens and the values metadata this side does not read.
    Ok(pairs
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .filter_map(|(_, f)| String::from_utf8(f).ok())
        .collect())
}

pub(crate) fn count(user: &str, channel: &str, outcome: &str, now: i64) {
    let Ok(url) = std::env::var("MAILRS_KEVY_URL") else {
        return;
    };
    let Ok(mut conn) = kevy_client::Connection::connect(&url) else {
        return;
    };
    let _ = mailrs_core_sidestate::families::notify::count(&mut conn, user, channel, outcome, now);
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

pub(crate) fn truncated(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_counts_characters_not_bytes() {
        let cjk = "件名".repeat(300);
        let cut = truncated(&cjk, 10);
        assert_eq!(cut.chars().count(), 10);
    }
}

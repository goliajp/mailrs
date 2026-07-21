//! Inbound importance scoring for the fastcore lane.
//!
//! RFC `.claude/rfcs/20260721-self-hosted-importance-ranking.md`.
//!
//! The heuristic scorer (`mailrs-intelligence`) and the relationship
//! facts (the shared side-state contacts hashes on network kevy) both
//! already existed; the monolith lane scored every inbound message with
//! them while this lane — the one actually serving prod — never did, so
//! every thread's importance stayed unset. This module closes that
//! parity gap.
//!
//! Signal derivation itself lives in the stone
//! (`importance::signals_for_inbound`) and is shared with the monolith,
//! so the two lanes cannot drift apart.

use std::sync::Arc;

use mailrs_intelligence::importance::{self, ContactFacts, MessageFacts};

use crate::FastcoreState;

/// Strip a display-name wrapper down to the bare address, lowercased.
fn bare_address(token: &str) -> String {
    let t = token.trim();
    let bare = match (t.rfind('<'), t.rfind('>')) {
        (Some(open), Some(close)) if close > open + 1 => &t[open + 1..close],
        _ => t,
    };
    bare.trim().to_lowercase()
}

/// First bare email address in a `senders_csv` value, lowercased.
///
/// The csv carries display-name forms (`Alice <a@x.com>, b@y.com`); the
/// contact hashes are keyed by the bare address.
fn first_address(senders_csv: &str) -> String {
    bare_address(senders_csv.split(',').next().unwrap_or(""))
}

/// Every bare address in a csv field, lowercased, empties dropped.
fn all_addresses(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(bare_address)
        .filter(|a| !a.is_empty() && a.contains('@'))
        .collect()
}

/// Message-shape signals, all from `mailrs-clean` pure helpers.
///
/// `head` is the header region, `raw` the whole message: the HTML
/// signals need the body, and a message with no HTML part contributes
/// no tracking-pixel / template / link evidence at all (matching the
/// monolith, which passes the plain-text branch through untouched).
///
/// `sender` must be the **bare** address: `is_automated_sender` matches
/// the local part exactly (`noreply`, `postmaster`, ...), so handing it
/// a `From:` header line silently never matches.
fn message_facts(sender: &str, head: &[u8], raw: &[u8]) -> MessageFacts {
    let headers = String::from_utf8_lossy(head);

    let mut facts = MessageFacts {
        is_bulk_sender: mailrs_clean::detect_bulk_sender(&headers),
        is_automated: mailrs_clean::is_automated_sender(sender),
        ..MessageFacts::default()
    };

    let root = mailrs_mime::parse(raw);
    let html = root
        .walk()
        .find(|p| p.content_type.mime_type().as_str() == "text/html")
        .and_then(|p| p.body_text());
    if let Some(html) = html {
        let cleaned = mailrs_clean::clean_email_html(&html);
        facts.has_tracking_pixel = cleaned.has_tracking_pixel;
        facts.is_template_heavy = cleaned.is_template_heavy;
        facts.link_count = cleaned.link_count;
    }
    facts
}

/// Score an inbound message and store the verdict on its thread row.
///
/// Best-effort: a missing network-kevy connection degrades to "no
/// relationship known" rather than skipping scoring, because the
/// message-shape signals alone still separate bulk mail from ordinary
/// mail. A store error is logged and swallowed — importance is a
/// derivation, and failing to write it must never fail delivery.
///
/// Call only for inbound messages. The user's own reply must not
/// restate the thread's importance, mirroring the display-field rule in
/// `record_message_arrival`.
pub(crate) fn score_inbound(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
    senders_csv: &str,
    head: &[u8],
    raw: &[u8],
) {
    let sender = first_address(senders_csv);
    if sender.is_empty() {
        return;
    }

    let facts = message_facts(&sender, head, raw);

    let (contact, is_reply_to_my_email) = match state.net_conn() {
        Some(mut conn) => {
            use mailrs_core_sidestate::families::contacts as ct;
            // Read the relationship BEFORE recording this message, so a
            // message never counts itself as evidence of a relationship.
            let s = ct::scoring_for(&mut conn, user, &sender);
            let replied = ct::has_sent_to_addr(&mut conn, user, &sender);

            // Capture the inbound fact. Until this landed, nothing in
            // this lane ever wrote the scoring hash, so is_mutual /
            // has_sent_to were permanently false (RFC §"prod 实测").
            if let Err(e) = ct::record_inbound(
                &mut conn,
                user,
                &sender,
                "",
                facts.is_bulk_sender,
                facts.is_automated,
            ) {
                tracing::warn!(error = %e, %user, %sender, "importance: inbound contact fact not recorded");
            }

            (
                Some(ContactFacts {
                    is_mutual: s.is_mutual,
                    is_vip: s.is_vip,
                    is_mailing_list: s.is_mailing_list,
                    importance_bias: s.importance_bias,
                }),
                replied,
            )
        }
        None => (None, false),
    };

    let signals = importance::signals_for_inbound(facts, contact, is_reply_to_my_email);
    let (level, score) = importance::calculate_importance(&signals);

    if let Err(e) = state
        .mailbox
        .set_thread_importance(thread_id, level.as_str(), score as f64)
    {
        tracing::warn!(error = %e, %user, %thread_id, "importance: store failed");
    }
}

/// Per-address message tallies for one user.
#[derive(Default, Clone, Copy)]
pub(crate) struct Tally {
    received: u64,
    sent: u64,
}

/// One-shot rebuild of the contact relationship counters from the
/// message history already in the store.
///
/// Why this is needed: nothing in this lane ever wrote the scoring hash,
/// so `is_mutual` / `has_sent_to` were permanently false. Capturing the
/// facts going forward (see [`score_inbound`] and the sender binary)
/// only lights up correspondents the user mails *after* the deploy —
/// years of existing relationships would stay invisible for months.
///
/// **Writes absolute counts, not increments.** HINCRBY would double the
/// tallies on a second run; deriving the total from the message history
/// and HSET-ing it makes the job idempotent, and the history is the
/// authority anyway — so a re-run after live traffic still lands on the
/// truth rather than clobbering it.
///
/// Returns `(users, addresses, messages_scanned)`.
pub(crate) fn backfill_relationships(state: &Arc<FastcoreState>) -> (u64, u64, u64) {
    use std::collections::HashMap;

    let users = state.mailbox.list_account_addresses().unwrap_or_default();
    let store = state.mailbox.store_ref();
    let (mut n_users, mut n_addrs, mut n_msgs) = (0u64, 0u64, 0u64);

    for user in &users {
        let mut tally: HashMap<String, Tally> = HashMap::new();
        let activity = mailrs_mailbox_kevy::keys::user_threads_by_activity(user);
        let Ok(entries) = store.zrevrange(activity.as_bytes(), 0, -1) else {
            continue;
        };
        for (tid_bytes, _) in entries {
            let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
                continue;
            };
            let Ok(wires) = state.mailbox.list_thread_messages(tid) else {
                continue;
            };
            for blob in wires {
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(&blob) else {
                    continue;
                };
                let sender = v.get("sender").and_then(|x| x.as_str()).unwrap_or("");
                let recipients = v.get("recipients").and_then(|x| x.as_str()).unwrap_or("");
                n_msgs += 1;

                // Direction is decided by whether the user is the sender,
                // matching how `is_own` is derived on the ingest path.
                if mailrs_mailbox_kevy::senders_csv_contains_user(sender, user) {
                    for to in all_addresses(recipients) {
                        tally.entry(to).or_default().sent += 1;
                    }
                } else {
                    let from = first_address(sender);
                    if !from.is_empty() && from.contains('@') {
                        tally.entry(from).or_default().received += 1;
                    }
                }
            }
        }
        if tally.is_empty() {
            continue;
        }
        let Some(mut conn) = state.net_conn() else {
            tracing::warn!("relationship backfill: no network kevy connection");
            return (n_users, n_addrs, n_msgs);
        };
        for (addr, t) in &tally {
            let key = format!("mailrs:contact:{user}:{addr}");
            let recv = t.received.to_string();
            let sent = t.sent.to_string();
            let pairs: [(&[u8], &[u8]); 2] = [
                (b"received_count", recv.as_bytes()),
                (b"sent_count", sent.as_bytes()),
            ];
            if let Err(e) = conn.hset(key.as_bytes(), &pairs) {
                tracing::warn!(error = %e, %user, %addr, "relationship backfill: hset failed");
                continue;
            }
            n_addrs += 1;
        }
        n_users += 1;
        tracing::info!(%user, addresses = tally.len(), "relationship backfill: user done");
    }
    (n_users, n_addrs, n_msgs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_addresses_splits_and_normalises() {
        assert_eq!(
            all_addresses("Alice <A@X.com>, bob@y.com"),
            vec!["a@x.com".to_string(), "bob@y.com".to_string()]
        );
        // Junk tokens without an '@' must not become contact keys.
        assert_eq!(
            all_addresses("undisclosed-recipients:;"),
            Vec::<String>::new()
        );
        assert_eq!(all_addresses(""), Vec::<String>::new());
    }

    #[test]
    fn extracts_bare_address_from_display_form() {
        assert_eq!(first_address("Alice <A@X.com>"), "a@x.com");
        assert_eq!(first_address("b@y.com"), "b@y.com");
        assert_eq!(first_address("Alice <a@x.com>, bob@y.com"), "a@x.com");
        assert_eq!(first_address(""), "");
    }

    #[test]
    fn bulk_headers_are_detected_as_message_facts() {
        let head =
            b"From: news@shop.example\r\nList-Unsubscribe: <https://x/u>\r\nSubject: Sale\r\n\r\n";
        let facts = message_facts("news@shop.example", head, head);
        assert!(facts.is_bulk_sender, "List-Unsubscribe implies bulk");
    }

    #[test]
    fn plain_message_has_no_html_signals() {
        let raw = b"From: alice@x.com\r\nSubject: hi\r\n\r\njust text\r\n";
        let facts = message_facts("alice@x.com", raw, raw);
        assert!(!facts.has_tracking_pixel);
        assert!(!facts.is_template_heavy);
        assert_eq!(facts.link_count, 0);
    }

    #[test]
    fn automated_sender_needs_the_bare_address() {
        // Regression: passing the whole `From:` header line makes
        // is_automated_sender split on '@' into "from: noreply", which
        // never matches its exact local-part list.
        let raw = b"From: noreply@x.com\r\nSubject: hi\r\n\r\ntext\r\n";
        assert!(
            message_facts("noreply@x.com", raw, raw).is_automated,
            "bare address must be recognised as automated"
        );
        assert!(
            !message_facts("From: noreply@x.com", raw, raw).is_automated,
            "header line must NOT match — proves we pass the bare address"
        );
    }
}

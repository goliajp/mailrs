//! Webhook subscriptions on network kevy — the storage routine, once.
//!
//! ```text
//!   admin:webhooks:{account}   hash  id → JSON WebhookSubWire
//!   admin:webhooks:counter     str   next id
//!   admin:webhooks:owner       hash  id → account address
//! ```
//!
//! Two handler sets wrote this namespace with their own copies of the same
//! four operations: `webapi/handlers/admin.rs` and this crate's
//! `admin_state`, mounted by fastcore. They agreed by accident and had no way
//! to notice when they stopped.
//!
//! The `owner` hash is new, and it is why delete works. A subscription is
//! keyed by account, and both copies found the account by reading
//! `mailrs:accounts:index` — a set v2.6.2 stopped writing and
//! `sweep-legacy-admin-keys` has since deleted. `smembers` of a missing key
//! is an empty list rather than an error, so the delete loop ran zero times
//! and answered 204. Deleting an admin webhook has been reporting success
//! without doing anything. Recording the owner at create time makes the
//! delete an exact lookup and removes the enumeration altogether — which is
//! what the code comment left behind ("we can index (id -> account)
//! separately") had proposed.

use kevy_client::Connection;
use mailrs_core_api::method::admin::WebhookSubWire;

const PREFIX: &str = "admin:webhooks:";
const COUNTER: &[u8] = b"admin:webhooks:counter";
const OWNER: &[u8] = b"admin:webhooks:owner";

fn account_key(address: &str) -> String {
    format!("{PREFIX}{address}")
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A fresh signing secret, base64.
pub fn new_signing_secret() -> String {
    use base64::Engine as _;
    let mut bytes = [0u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// What a caller supplies to create a subscription.
pub struct NewWebhook {
    /// The account the subscription belongs to.
    pub account_address: String,
    /// Where to POST.
    pub url: String,
    /// Which event fires it.
    pub event_type: String,
    /// Only fire for mail from this address.
    pub filter_sender: Option<String>,
    /// Only fire for this conversation.
    pub filter_thread_id: Option<String>,
}

/// Store a subscription and record who owns it. Returns the row as stored.
pub fn create(conn: &mut Connection, new: NewWebhook) -> std::io::Result<WebhookSubWire> {
    let id = conn.incr(COUNTER)?;
    let w = WebhookSubWire {
        id,
        account_address: new.account_address.clone(),
        url: new.url,
        event_type: new.event_type,
        filter_sender: new.filter_sender,
        filter_thread_id: new.filter_thread_id,
        signing_secret: new_signing_secret(),
        active: true,
        created_at: now_secs(),
    };
    let json = serde_json::to_vec(&w)?;
    let id_str = id.to_string();
    conn.hset(
        account_key(&new.account_address).as_bytes(),
        &[(id_str.as_bytes(), json.as_slice())],
    )?;
    // Written in the same call as the row, because a row without an owner
    // entry is one that cannot be deleted.
    conn.hset(
        OWNER,
        &[(id_str.as_bytes(), new.account_address.as_bytes())],
    )?;
    Ok(w)
}

/// Every subscription belonging to one account.
pub fn list(conn: &mut Connection, address: &str) -> std::io::Result<Vec<WebhookSubWire>> {
    let flat = conn.hgetall(account_key(address).as_bytes())?;
    Ok(flat
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| match i % 2 {
            1 => Some(v),
            _ => None,
        })
        .filter_map(|v| serde_json::from_slice::<WebhookSubWire>(&v).ok())
        .collect())
}

/// Remove a subscription. Returns whether one was there to remove.
///
/// The bool is the point: the previous implementations answered 204 whether
/// or not anything happened, so a delete against a swept index was
/// indistinguishable from a successful one.
pub fn delete(conn: &mut Connection, id: i64) -> std::io::Result<bool> {
    let id_str = id.to_string();
    let owner = conn.hget(OWNER, id_str.as_bytes())?;
    let Some(owner) = owner else {
        return Ok(false);
    };
    // not a KevyError, so it carries no engine category to preserve
    let address = String::from_utf8(owner).map_err(std::io::Error::other)?;
    let removed = conn.hdel(account_key(&address).as_bytes(), &[id_str.as_bytes()])?;
    conn.hdel(OWNER, &[id_str.as_bytes()])?;
    Ok(removed > 0)
}

/// Give every pre-existing subscription an owner entry.
///
/// Rows created before the owner hash existed have none, so `delete` cannot
/// find them — the same silent no-op, for a different reason. `addresses` is
/// the account list the caller can reach; each is read once and its rows
/// recorded. Returns how many entries were added and how many accounts were
/// examined, because a zero from the first is only meaningful next to the
/// second.
pub fn backfill_owner_index(
    conn: &mut Connection,
    addresses: &[String],
) -> std::io::Result<(usize, usize)> {
    let mut added = 0usize;
    for address in addresses {
        for w in list(conn, address)? {
            let id_str = w.id.to_string();
            let known = conn.hget(OWNER, id_str.as_bytes())?;
            if known.is_some() {
                continue;
            }
            conn.hset(OWNER, &[(id_str.as_bytes(), address.as_bytes())])?;
            added += 1;
        }
    }
    Ok((added, addresses.len()))
}

/// One retired-namespace row, as a subscription in the surviving one.
///
/// Pure, because this is where a migration goes quietly wrong: dropping the
/// signing secret breaks every subscriber's verification, and dropping a
/// filter turns a webhook scoped to one sender into one that fires on
/// everything — neither shows up as an error anywhere.
fn row_from_legacy(old: &serde_json::Value, id: i64, address: &str) -> WebhookSubWire {
    WebhookSubWire {
        id,
        account_address: address.to_string(),
        url: old["url"].as_str().unwrap_or_default().to_string(),
        event_type: old["event_type"].as_str().unwrap_or_default().to_string(),
        filter_sender: old["filter_sender"].as_str().map(str::to_string),
        filter_thread_id: old["filter_thread_id"].as_str().map(str::to_string),
        // Kept: the subscriber verifies signatures with it, and a new secret
        // would silently break every delivery.
        signing_secret: old["signing_secret"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(new_signing_secret),
        active: old["active"].as_bool().unwrap_or(true),
        created_at: old["created_at"].as_i64().unwrap_or_else(now_secs),
    }
}

/// Move rows out of the retired `agent:webhooks:{user}` namespace.
///
/// The settings page wrote there and the admin surface wrote
/// `admin:webhooks:{account}`, so a subscription was visible to exactly one
/// of the two. Ids came from separate counters, so a row's own id may
/// already be taken here — it is reallocated, and the signing secret is
/// preserved because the subscriber has it.
///
/// Returns rows moved and accounts examined. Idempotent: the source hash is
/// deleted as each account is done.
pub fn migrate_agent_namespace(
    conn: &mut Connection,
    addresses: &[String],
) -> std::io::Result<(usize, usize)> {
    let mut moved = 0usize;
    for address in addresses {
        let legacy_key = format!("agent:webhooks:{address}");
        let flat = conn.hgetall(legacy_key.as_bytes())?;
        let rows: Vec<Vec<u8>> = flat
            .into_iter()
            .enumerate()
            .filter_map(|(i, v)| match i % 2 {
                1 => Some(v),
                _ => None,
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        for raw in rows {
            let Ok(old) = serde_json::from_slice::<serde_json::Value>(&raw) else {
                continue;
            };
            let id = conn.incr(COUNTER)?;
            let w = row_from_legacy(&old, id, address);
            let json = serde_json::to_vec(&w)?;
            let id_str = id.to_string();
            conn.hset(
                account_key(address).as_bytes(),
                &[(id_str.as_bytes(), json.as_slice())],
            )?;
            conn.hset(OWNER, &[(id_str.as_bytes(), address.as_bytes())])?;
            moved += 1;
        }
        conn.del(&[legacy_key.as_bytes()])?;
        let counter = format!("agent:webhooks:counter:{address}");
        conn.del(&[counter.as_bytes()])?;
    }
    Ok((moved, addresses.len()))
}

/// Whether a subscription's filters admit this message.
///
/// Addresses are compared by [`mailrs_rfc5322::addr_key`], not by string
/// equality. The monolith's matcher used `!=` on the raw values, so a filter
/// stored as `a@b.com` never matched a sender header written
/// `Name <a@b.com>` — the form most mail actually arrives in, which made a
/// sender-filtered subscription silently fire for nobody.
pub fn matches(sub: &WebhookSubWire, sender: &str, thread_id: &str) -> bool {
    if !sub.active {
        return false;
    }
    if let Some(ref f) = sub.filter_sender
        && mailrs_rfc5322::addr_key(f) != mailrs_rfc5322::addr_key(sender)
    {
        return false;
    }
    if let Some(ref f) = sub.filter_thread_id
        && f != thread_id
    {
        return false;
    }
    true
}

/// The body POSTed for a newly arrived message.
///
/// `timestamp` is passed in rather than read from the clock so the payload
/// is assertable.
pub fn new_message_payload(
    user: &str,
    thread_id: &str,
    sender: &str,
    subject: &str,
    snippet: &str,
    timestamp: &str,
) -> serde_json::Value {
    serde_json::json!({
        "event": "new_message",
        "timestamp": timestamp,
        "data": {
            "user": user,
            "thread_id": thread_id,
            "sender": sender,
            "subject": subject,
            "snippet": snippet,
        }
    })
}

/// The subscriptions of `address` that admit this message.
pub fn matching(
    conn: &mut Connection,
    address: &str,
    sender: &str,
    thread_id: &str,
) -> std::io::Result<Vec<WebhookSubWire>> {
    Ok(list(conn, address)?
        .into_iter()
        .filter(|s| s.event_type == "new_message" && matches(s, sender, thread_id))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_scoped_to_the_account() {
        assert_eq!(account_key("a@b.com"), "admin:webhooks:a@b.com");
    }

    /// The migration must not invent a secret: the subscriber already holds
    /// the old one and verifies every delivery against it.
    #[test]
    fn migration_keeps_the_signing_secret_and_the_filters() {
        let old = serde_json::json!({
            "id": 3,
            "url": "https://example.com/hook",
            "event_type": "mail.received",
            "filter_sender": "nagata@nagatax.tokyo.jp",
            "filter_thread_id": null,
            "signing_secret": "the-secret-the-subscriber-has",
            "created_at": 1_785_000_000_i64,
            "active": true,
        });
        let w = row_from_legacy(&old, 91, "lihao@golia.jp");

        assert_eq!(w.signing_secret, "the-secret-the-subscriber-has");
        assert_eq!(w.filter_sender.as_deref(), Some("nagata@nagatax.tokyo.jp"));
        assert_eq!(w.filter_thread_id, None);
        assert_eq!(w.created_at, 1_785_000_000);
        // Reallocated: the two namespaces had separate counters, so the old
        // id may already belong to something else here.
        assert_eq!(w.id, 91);
        assert_eq!(w.account_address, "lihao@golia.jp");
    }

    fn sub(filter_sender: Option<&str>, filter_thread_id: Option<&str>) -> WebhookSubWire {
        WebhookSubWire {
            id: 1,
            account_address: "lihao@golia.jp".into(),
            url: "https://example.com/hook".into(),
            event_type: "new_message".into(),
            filter_sender: filter_sender.map(str::to_string),
            filter_thread_id: filter_thread_id.map(str::to_string),
            signing_secret: "s".into(),
            active: true,
            created_at: 0,
        }
    }

    /// The defect the shared matcher replaces: the monolith compared the
    /// stored filter to the sender header with `!=`, and mail arrives as
    /// `Name <a@b.com>`. A subscription scoped to one sender fired for
    /// nobody, which looks exactly like a quiet mailbox.
    #[test]
    fn a_sender_filter_matches_either_mailbox_form() {
        let s = sub(Some("nagata@nagatax.tokyo.jp"), None);
        assert!(matches(&s, "nagata@nagatax.tokyo.jp", "t1"));
        assert!(matches(&s, "Nagata <Nagata@NagataX.Tokyo.JP>", "t1"));
        // Still not a substring match.
        assert!(!matches(&s, "notnagata@nagatax.tokyo.jp", "t1"));
    }

    #[test]
    fn no_filter_admits_everything_and_a_thread_filter_binds() {
        assert!(matches(&sub(None, None), "anyone@x.com", "any"));
        let t = sub(None, Some("t1"));
        assert!(matches(&t, "anyone@x.com", "t1"));
        assert!(!matches(&t, "anyone@x.com", "t2"));
    }

    #[test]
    fn an_inactive_subscription_never_matches() {
        let mut s = sub(None, None);
        s.active = false;
        assert!(!matches(&s, "anyone@x.com", "t1"));
    }

    #[test]
    fn the_payload_names_the_event_the_worker_reads() {
        let p = new_message_payload("u@x", "t1", "a@b", "Subj", "snip", "2026-07-31T00:00:00Z");
        // The worker reads `event` for the X-Mailrs-Event header.
        assert_eq!(p["event"], "new_message");
        assert_eq!(p["data"]["thread_id"], "t1");
        assert_eq!(p["data"]["subject"], "Subj");
    }

    /// A row missing its secret gets a fresh one rather than an empty
    /// string, which would sign every delivery with nothing.
    #[test]
    fn a_row_without_a_secret_gets_one() {
        let old = serde_json::json!({ "url": "https://x", "event_type": "e" });
        let w = row_from_legacy(&old, 1, "a@b.com");
        assert!(!w.signing_secret.is_empty());
        assert!(w.active, "a row that did not say defaults to active");
    }
}

//! A reader's rules for one sender: which bucket their mail goes to, and
//! how long it is kept.
//!
//! ```text
//! sender_rules:{user}          hash  address -> rule JSON
//! sender_rules:retaining       set   users with at least one retention rule
//! corrections:{user}           list  correction and undo facts, oldest first
//! ```
//!
//! A rule is the reader's preference and is edited in place. A correction
//! (moving a sender's conversations) is a fact: appended, never changed;
//! undoing it appends another.

use serde::{Deserialize, Serialize};

/// The buckets a sender's mail can be moved between.
pub const BUCKETS: [&str; 3] = ["inbox", "notifications", "promotions"];

pub fn rules_key(user: &str) -> String {
    format!("sender_rules:{}", user.to_lowercase())
}

/// The users the retention sweep visits; nobody in it, nothing to do.
pub const RETAINING_KEY: &str = "sender_rules:retaining";

pub fn corrections_key(user: &str) -> String {
    format!("corrections:{}", user.to_lowercase())
}

/// What happens to a conversation a retention rule lets go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposal {
    /// Out of the Inbox, still found by search and restorable.
    #[default]
    Archive,
    /// To the Trash, deleted for good by the Trash's own purge.
    Trash,
}

/// One sender's rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderRule {
    /// `inbox`, `notifications` or `promotions`; `None` keeps classification
    /// to the classifier and leaves only the retention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    /// Keep only this many of the sender's newest conversations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_latest: Option<u32>,
    /// Let go of the sender's conversations older than this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older_than_days: Option<u32>,
    #[serde(default)]
    pub disposal: Disposal,
    /// `correction` or `settings`.
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub created_at: i64,
}

impl SenderRule {
    pub fn retains(&self) -> bool {
        self.keep_latest.is_some() || self.older_than_days.is_some()
    }
}

/// The address as rules are keyed: the addr-spec, lowercased.
pub fn normalize(address: &str) -> String {
    mailrs_rfc5322::addr_spec(address)
        .trim()
        .to_ascii_lowercase()
}

/// `user`'s rule for `sender`, if any.
pub fn get(
    conn: &mut kevy_client::Connection,
    user: &str,
    sender: &str,
) -> std::io::Result<Option<SenderRule>> {
    let raw = conn.hget(rules_key(user).as_bytes(), normalize(sender).as_bytes())?;
    Ok(raw.and_then(|v| serde_json::from_slice(&v).ok()))
}

/// Every rule of `user`, by address.
pub fn list(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> std::io::Result<Vec<(String, SenderRule)>> {
    let flat = conn.hgetall(rules_key(user).as_bytes())?;
    let mut out: Vec<(String, SenderRule)> = flat
        .chunks_exact(2)
        .filter_map(|p| {
            let rule = serde_json::from_slice(&p[1]).ok()?;
            Some((String::from_utf8_lossy(&p[0]).into_owned(), rule))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Write `user`'s rule for `sender`, keeping the retention set in step.
pub fn put(
    conn: &mut kevy_client::Connection,
    user: &str,
    sender: &str,
    rule: &SenderRule,
) -> std::io::Result<()> {
    let body = serde_json::to_vec(rule).map_err(std::io::Error::other)?;
    conn.hset(
        rules_key(user).as_bytes(),
        &[(normalize(sender).as_bytes(), body.as_slice())],
    )?;
    sync_retaining(conn, user)
}

/// Remove `user`'s rule for `sender`; whether there was one.
pub fn delete(
    conn: &mut kevy_client::Connection,
    user: &str,
    sender: &str,
) -> std::io::Result<bool> {
    let n = conn.hdel(rules_key(user).as_bytes(), &[normalize(sender).as_bytes()])?;
    sync_retaining(conn, user)?;
    Ok(n > 0)
}

fn sync_retaining(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<()> {
    let any = list(conn, user)?.iter().any(|(_, r)| r.retains());
    let member = user.to_lowercase();
    match any {
        true => conn.sadd(RETAINING_KEY.as_bytes(), &[member.as_bytes()])?,
        false => conn.srem(RETAINING_KEY.as_bytes(), &[member.as_bytes()])?,
    };
    Ok(())
}

/// The users with a retention rule.
pub fn retaining_users(conn: &mut kevy_client::Connection) -> std::io::Result<Vec<String>> {
    Ok(conn
        .smembers(RETAINING_KEY.as_bytes())?
        .into_iter()
        .map(|m| String::from_utf8_lossy(&m).into_owned())
        .collect())
}

/// One conversation a correction moved, and the bucket it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    pub thread_id: String,
    pub from_bucket: String,
}

/// A correction, or the undoing of one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Correction {
    Moved {
        id: String,
        sender: String,
        to_bucket: String,
        moved: Vec<Moved>,
        at: i64,
    },
    Undone {
        id: String,
        at: i64,
    },
}

/// Append one fact.
pub fn record(
    conn: &mut kevy_client::Connection,
    user: &str,
    fact: &Correction,
) -> std::io::Result<()> {
    let body = serde_json::to_vec(fact).map_err(std::io::Error::other)?;
    conn.rpush(corrections_key(user).as_bytes(), &[body.as_slice()])?;
    Ok(())
}

/// The correction `id`, unless it has been undone.
pub fn open_correction(
    conn: &mut kevy_client::Connection,
    user: &str,
    id: &str,
) -> std::io::Result<Option<Correction>> {
    let facts: Vec<Correction> = conn
        .lrange(corrections_key(user).as_bytes(), 0, -1)?
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    let undone = facts
        .iter()
        .any(|f| matches!(f, Correction::Undone { id: u, .. } if u == id));
    if undone {
        return Ok(None);
    }
    Ok(facts
        .into_iter()
        .find(|f| matches!(f, Correction::Moved { id: m, .. } if m == id)))
}

#[cfg(test)]
mod tests;

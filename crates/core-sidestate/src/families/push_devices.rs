//! Every device an account receives pushes on, across APNs, Web Push and
//! UnifiedPush, held to [`MAX_DEVICES`] together.
//!
//! ```text
//! push:tokens:{user}         hash  APNs token -> {"platform","registered_at"}
//! push:web:{user}            hash  sha256(endpoint) -> WebSubscription JSON
//! push:unifiedpush:{user}    hash  sha256(endpoint) -> Device JSON
//! push:used:{user}           hash  "{channel}:{id}" -> epoch seconds of the last push that got through
//! push:session:{user}        hash  "{channel}:{id}" -> ref of the session that last registered it
//! push:removed:{user}        hash  "{channel}:{id}" -> epoch seconds a person removed it; fields expire
//! ```
//!
//! Each registration path goes through [`register`], which files the
//! device and then evicts the least recently used others until the account
//! is back at the cap. A device's last use is the later of its last push
//! that got through and its last registration. How devices leave other
//! than by the cap is in the `removal` module.

use std::collections::HashMap;

use super::notify::{self, WebSubscription};
use super::unified_push::{self, Device};
use crate::keys::push::{session_link_key, tokens_key, used_key, web_subscriptions_key};

mod removal;
pub use removal::{REMOVED_KEPT, drop_all, drop_for_session, remove, removed};

/// The most devices one account keeps, all channels together.
pub const MAX_DEVICES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    Apns,
    Web,
    UnifiedPush,
}

impl Channel {
    /// The name the push counters use.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apns => "apns",
            Self::Web => "web",
            Self::UnifiedPush => "unifiedpush",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "apns" => Some(Self::Apns),
            "web" => Some(Self::Web),
            "unifiedpush" => Some(Self::UnifiedPush),
            _ => None,
        }
    }
}

/// A device asking to receive pushes.
#[derive(Debug, Clone, Copy)]
pub enum Registration<'a> {
    Apns { token: &'a str, platform: &'a str },
    Web(&'a WebSubscription),
    UnifiedPush(&'a Device),
}

impl Registration<'_> {
    /// The channel and id the device is filed under.
    pub fn identity(&self) -> (Channel, String) {
        match self {
            Self::Apns { token, .. } => (Channel::Apns, (*token).to_string()),
            Self::Web(sub) => (Channel::Web, notify::endpoint_field(&sub.endpoint)),
            Self::UnifiedPush(device) => {
                (Channel::UnifiedPush, unified_push::id_of(&device.endpoint))
            }
        }
    }
}

/// Who asks for a registration.
#[derive(Debug, Clone, Copy, Default)]
pub struct Asker<'a> {
    /// The ref of the session the request came with; none for an agent key.
    pub session: Option<&'a str>,
    /// The person turned pushes on on this device just now. Only this lets
    /// a device a person removed elsewhere register again: the apps also
    /// register on their own (every launch, after finding themselves gone),
    /// and those must not undo a removal.
    pub resume: bool,
}

/// What [`register`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filed {
    Registered(Registered),
    /// A person removed this device from another one, and this request is
    /// not the device's own person turning pushes on again.
    Removed,
}

/// A device [`register`] filed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    /// The device's id within its channel.
    pub id: String,
    /// The devices removed to make room.
    pub evicted: Vec<(Channel, String)>,
}

/// One registered device, as the cap sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Held {
    channel: Channel,
    id: String,
    created_at: i64,
}

fn used_field(channel: Channel, id: &str) -> String {
    format!("{}:{id}", channel.as_str())
}

/// File `reg` for `user`, unless a person removed the device and `by` does
/// not resume it; then evict the least recently used other devices until
/// the account holds at most [`MAX_DEVICES`]. Registering a device already
/// held replaces it and counts once.
///
/// Not atomic across the three hashes: two registrations racing can each
/// evict one, or the same one, leaving the account one under or one over
/// the cap until its next registration.
///
/// # Errors
/// The store could not be read or written.
pub fn register(
    conn: &mut kevy_client::Connection,
    user: &str,
    reg: Registration<'_>,
    by: Asker<'_>,
    now: i64,
) -> std::io::Result<Filed> {
    let (channel, id) = reg.identity();
    if removal::is_removed(conn, user, channel, &id)? {
        if !by.resume {
            return Ok(Filed::Removed);
        }
        removal::forget_removal(conn, user, channel, &id)?;
    }
    file(conn, user, reg, now)?;
    removal::link(conn, user, channel, &id, by.session)?;
    let evicted = enforce_cap(conn, user, channel, &id)?;
    Ok(Filed::Registered(Registered { id, evicted }))
}

fn file(
    conn: &mut kevy_client::Connection,
    user: &str,
    reg: Registration<'_>,
    now: i64,
) -> std::io::Result<()> {
    match reg {
        Registration::Apns { token, platform } => {
            let meta =
                serde_json::json!({ "platform": platform, "registered_at": now }).to_string();
            conn.hset(
                tokens_key(user).as_bytes(),
                &[(token.as_bytes(), meta.as_bytes())],
            )?;
        }
        Registration::Web(sub) => notify::add_web_subscription(conn, user, sub)?,
        Registration::UnifiedPush(device) => {
            unified_push::add(conn, user, device)?;
        }
    }
    Ok(())
}

/// Record that a push to `id` on `channel` got through.
///
/// # Errors
/// The store could not be written.
pub fn mark_used(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: Channel,
    id: &str,
    now: i64,
) -> std::io::Result<()> {
    let field = used_field(channel, id);
    conn.hset(
        used_key(user).as_bytes(),
        &[(field.as_bytes(), now.to_string().as_bytes())],
    )?;
    Ok(())
}

/// Every device `user` holds, on every channel.
fn held(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Vec<Held>> {
    let mut out = Vec::new();
    for pair in conn.hgetall(tokens_key(user).as_bytes())?.chunks_exact(2) {
        // a token whose metadata does not parse counts as the oldest
        let created_at = serde_json::from_slice::<serde_json::Value>(&pair[1])
            .ok()
            .and_then(|v| v["registered_at"].as_i64())
            .unwrap_or(0);
        out.push(Held {
            channel: Channel::Apns,
            id: String::from_utf8_lossy(&pair[0]).into_owned(),
            created_at,
        });
    }
    for pair in conn
        .hgetall(web_subscriptions_key(user).as_bytes())?
        .chunks_exact(2)
    {
        let created_at = serde_json::from_slice::<WebSubscription>(&pair[1])
            .map(|s| s.created_at)
            .unwrap_or(0);
        out.push(Held {
            channel: Channel::Web,
            id: String::from_utf8_lossy(&pair[0]).into_owned(),
            created_at,
        });
    }
    for (id, device) in unified_push::list(conn, user)? {
        out.push(Held {
            channel: Channel::UnifiedPush,
            id,
            created_at: device.created_at,
        });
    }
    Ok(out)
}

fn last_used(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> std::io::Result<HashMap<String, i64>> {
    Ok(conn
        .hgetall(used_key(user).as_bytes())?
        .chunks_exact(2)
        .map(|p| {
            let at = String::from_utf8_lossy(&p[1]).parse().unwrap_or(0);
            (String::from_utf8_lossy(&p[0]).into_owned(), at)
        })
        .collect())
}

/// Take the device out of its channel's hash; whether it was there.
fn evict(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: Channel,
    id: &str,
) -> std::io::Result<bool> {
    let gone = match channel {
        Channel::Apns => conn.hdel(tokens_key(user).as_bytes(), &[id.as_bytes()])? > 0,
        Channel::Web => conn.hdel(web_subscriptions_key(user).as_bytes(), &[id.as_bytes()])? > 0,
        Channel::UnifiedPush => unified_push::remove(conn, user, id)?,
    };
    Ok(gone)
}

/// Evict down to the cap, sparing `keep`, and drop the last-used times and
/// session links of devices no longer held.
fn enforce_cap(
    conn: &mut kevy_client::Connection,
    user: &str,
    keep_channel: Channel,
    keep_id: &str,
) -> std::io::Result<Vec<(Channel, String)>> {
    let devices = held(conn, user)?;
    let used = last_used(conn, user)?;
    let mut ranked: Vec<(i64, Channel, String)> = devices
        .iter()
        .filter(|d| !(d.channel == keep_channel && d.id == keep_id))
        .map(|d| {
            // a registration counts as use too: iOS registers again on
            // every launch, and a phone opened daily but rarely pushed to
            // is not the unused one
            let at = used
                .get(&used_field(d.channel, &d.id))
                .copied()
                .map_or(d.created_at, |u| u.max(d.created_at));
            (at, d.channel, d.id.clone())
        })
        .collect();
    ranked.sort();
    let over = devices.len().saturating_sub(MAX_DEVICES);
    let mut evicted = Vec::with_capacity(over);
    for (_, channel, id) in ranked.into_iter().take(over) {
        evict(conn, user, channel, &id)?;
        evicted.push((channel, id));
    }
    let live: std::collections::HashSet<String> = devices
        .iter()
        .filter(|d| !evicted.contains(&(d.channel, d.id.clone())))
        .map(|d| used_field(d.channel, &d.id))
        .collect();
    for key in [used_key(user), session_link_key(user)] {
        drop_fields_not_in(conn, &key, &live)?;
    }
    Ok(evicted)
}

fn drop_fields_not_in(
    conn: &mut kevy_client::Connection,
    key: &str,
    live: &std::collections::HashSet<String>,
) -> std::io::Result<()> {
    let fields = conn.hgetall(key.as_bytes())?;
    let stale: Vec<&[u8]> = fields
        .chunks_exact(2)
        .map(|p| p[0].as_slice())
        .filter(|f| !live.contains(String::from_utf8_lossy(f).as_ref()))
        .collect();
    if !stale.is_empty() {
        conn.hdel(key.as_bytes(), &stale)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

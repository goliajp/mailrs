//! Every device an account receives pushes on, across APNs, Web Push and
//! UnifiedPush, held to [`MAX_DEVICES`] together.
//!
//! ```text
//! push:tokens:{user}         hash  APNs token -> {"platform","registered_at"}
//! push:web:{user}            hash  sha256(endpoint) -> WebSubscription JSON
//! push:unifiedpush:{user}    hash  sha256(endpoint) -> Device JSON
//! push:used:{user}           hash  "{channel}:{id}" -> epoch seconds of the last push that got through
//! ```
//!
//! Each registration path goes through [`register`], which files the
//! device and then evicts the least recently used others until the account
//! is back at the cap. A device that never took a push counts from when it
//! was registered.

use std::collections::HashMap;

use super::notify::{self, WebSubscription};
use super::unified_push::{self, Device};
use crate::keys::push::{tokens_key, used_key, web_subscriptions_key};

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
}

/// A device asking to receive pushes.
#[derive(Debug, Clone, Copy)]
pub enum Registration<'a> {
    Apns { token: &'a str, platform: &'a str },
    Web(&'a WebSubscription),
    UnifiedPush(&'a Device),
}

/// What [`register`] did.
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

/// File `reg` for `user`, then evict the least recently used other devices
/// until the account holds at most [`MAX_DEVICES`]. Registering a device
/// already held replaces it and counts once.
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
    now: i64,
) -> std::io::Result<Registered> {
    let (channel, id) = file(conn, user, reg, now)?;
    let evicted = enforce_cap(conn, user, channel, &id)?;
    Ok(Registered { id, evicted })
}

fn file(
    conn: &mut kevy_client::Connection,
    user: &str,
    reg: Registration<'_>,
    now: i64,
) -> std::io::Result<(Channel, String)> {
    match reg {
        Registration::Apns { token, platform } => {
            let meta =
                serde_json::json!({ "platform": platform, "registered_at": now }).to_string();
            conn.hset(
                tokens_key(user).as_bytes(),
                &[(token.as_bytes(), meta.as_bytes())],
            )?;
            Ok((Channel::Apns, token.to_string()))
        }
        Registration::Web(sub) => {
            notify::add_web_subscription(conn, user, sub)?;
            Ok((Channel::Web, notify::endpoint_field(&sub.endpoint)))
        }
        Registration::UnifiedPush(device) => {
            let id = unified_push::add(conn, user, device)?;
            Ok((Channel::UnifiedPush, id))
        }
    }
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

fn evict(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: Channel,
    id: &str,
) -> std::io::Result<()> {
    match channel {
        Channel::Apns => {
            conn.hdel(tokens_key(user).as_bytes(), &[id.as_bytes()])?;
        }
        Channel::Web => {
            conn.hdel(web_subscriptions_key(user).as_bytes(), &[id.as_bytes()])?;
        }
        Channel::UnifiedPush => {
            unified_push::remove(conn, user, id)?;
        }
    }
    Ok(())
}

/// Evict down to the cap, sparing `keep`, and drop last-used times of
/// devices no longer held.
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
            let at = used
                .get(&used_field(d.channel, &d.id))
                .copied()
                .unwrap_or(d.created_at);
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
    let stale: Vec<&String> = used.keys().filter(|f| !live.contains(*f)).collect();
    if !stale.is_empty() {
        let fields: Vec<&[u8]> = stale.iter().map(|f| f.as_bytes()).collect();
        conn.hdel(used_key(user).as_bytes(), &fields)?;
    }
    Ok(evicted)
}

#[cfg(test)]
mod tests;

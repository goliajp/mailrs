//! When this deployment first saw a domain.
//!
//! ```text
//! mailrs:domfirst:{registrable_domain}   integer   Unix seconds, written once
//! mailrs:domfirst-since                  integer   when recording began, written once
//! ```
//!
//! A delivered message records its sender's registrable domain and those of
//! its links. Each is a fact: [`record`] is the only writer and never
//! changes a time already there, so no re-judging, rescan or replay can move
//! it. The network store has no `SET NX`; the time is read and written in one
//! connection, so two deliveries racing for the same new domain write times
//! a moment apart, and the first to land stands.
//!
//! Deployment-wide, like `mailrs:domseen:`: whether a domain is new is a
//! property of the install, and one key per domain keeps it cheap. Spam with
//! random link domains adds keys, so [`prune`] lets go of the ones seen only
//! once and long ago; one that comes back is recorded again as new, which is
//! what it is.

use std::io;

const PREFIX: &str = "mailrs:domfirst:";
const SINCE: &[u8] = b"mailrs:domfirst-since";
const PATTERN: &[u8] = b"mailrs:domfirst:*";
/// Link domains a single message records at most.
pub const LINKS_PER_MESSAGE: usize = 50;
/// A domain seen once is let go of this long after it was first seen.
pub const PRUNE_AFTER: i64 = 180 * 86_400;

/// The key of `registrable`'s first-seen time.
pub fn key(registrable: &str) -> String {
    format!("{PREFIX}{registrable}")
}

fn number(v: Option<Vec<u8>>) -> Option<i64> {
    v.and_then(|b| String::from_utf8_lossy(&b).trim().parse().ok())
}

fn write_once(conn: &mut kevy_client::Connection, k: &[u8], now: i64) -> io::Result<()> {
    if conn.exists(&[k])? > 0 {
        return Ok(());
    }
    conn.set(k, now.to_string().as_bytes())?;
    Ok(())
}

/// Record `hosts` (the sender's first, then the link hosts) as seen at
/// `now`, by registrable domain, unless already recorded. At most
/// [`LINKS_PER_MESSAGE`] beyond the first.
pub fn record(conn: &mut kevy_client::Connection, hosts: &[String], now: i64) -> io::Result<()> {
    write_once(conn, SINCE, now)?;
    let mut done: Vec<String> = Vec::new();
    for host in hosts.iter().take(LINKS_PER_MESSAGE + 1) {
        let reg = mailrs_fraud::brand::registrable(host.trim().trim_end_matches('.'));
        if reg.is_empty() || done.contains(&reg) {
            continue;
        }
        write_once(conn, key(&reg).as_bytes(), now)?;
        done.push(reg);
    }
    Ok(())
}

/// When recording began, if it has.
pub fn since(conn: &mut kevy_client::Connection) -> Option<i64> {
    number(conn.get(SINCE).ok().flatten())
}

/// When `host`'s registrable domain was first seen, if it has been.
pub fn first_seen(conn: &mut kevy_client::Connection, host: &str) -> Option<i64> {
    let reg = mailrs_fraud::brand::registrable(host.trim().trim_end_matches('.'));
    if reg.is_empty() {
        return None;
    }
    number(conn.get(key(&reg).as_bytes()).ok().flatten())
}

/// The novelty of a message from `sender_host` with links to `link_hosts`,
/// arriving at `at`. Not new at all without a store.
pub fn novelty(
    conn: Option<&mut kevy_client::Connection>,
    sender_host: &str,
    link_hosts: &[String],
    at: i64,
) -> mailrs_fraud::novelty::Novelty {
    let Some(conn) = conn else {
        return Default::default();
    };
    let since = since(conn);
    let sender = (
        first_seen(conn, sender_host),
        super::domain_history::seen(conn, sender_host),
    );
    let mut links: Vec<(String, Option<i64>)> = Vec::new();
    for host in link_hosts.iter().take(LINKS_PER_MESSAGE) {
        let reg = mailrs_fraud::brand::registrable(host.trim().trim_end_matches('.'));
        if reg.is_empty() || links.iter().any(|(d, _)| *d == reg) {
            continue;
        }
        let first = number(conn.get(key(&reg).as_bytes()).ok().flatten());
        links.push((reg, first));
    }
    mailrs_fraud::novelty::assess(sender, &links, since, at)
}

/// Which of `hosts` belong to a domain new for a message that arrived at
/// `at`. None without a store.
pub fn new_hosts(
    conn: Option<&mut kevy_client::Connection>,
    hosts: &[String],
    at: i64,
) -> Vec<String> {
    let Some(conn) = conn else {
        return Vec::new();
    };
    let since = since(conn);
    hosts
        .iter()
        .take(LINKS_PER_MESSAGE)
        .filter(|h| mailrs_fraud::novelty::is_new(first_seen(conn, h), 0, since, at))
        .cloned()
        .collect()
}

/// Every recorded domain with its first-seen time.
fn all(conn: &mut kevy_client::Connection) -> io::Result<Vec<(Vec<u8>, Option<i64>)>> {
    let mut keys: Vec<Vec<u8>> = Vec::new();
    let mut cursor = 0u64;
    loop {
        let (next, batch) = conn.scan(cursor, Some(PATTERN), Some(1000))?;
        keys.extend(batch);
        if next == 0 {
            break;
        }
        cursor = next;
    }
    let mut out = Vec::with_capacity(keys.len());
    for chunk in keys.chunks(500) {
        let refs: Vec<&[u8]> = chunk.iter().map(|k| k.as_slice()).collect();
        let values = conn.mget(&refs)?;
        for (k, v) in chunk.iter().zip(values) {
            out.push((k.clone(), number(v)));
        }
    }
    Ok(out)
}

/// How many domains are recorded, and how many of them were first seen in
/// the 24 hours before `now`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Census {
    pub keys: u64,
    pub added_24h: u64,
}

/// [`Census`] at `now`.
pub fn census(conn: &mut kevy_client::Connection, now: i64) -> io::Result<Census> {
    let rows = all(conn)?;
    let added_24h = rows
        .iter()
        .filter(|(_, first)| first.is_some_and(|f| now - f < 86_400))
        .count() as u64;
    Ok(Census {
        keys: rows.len() as u64,
        added_24h,
    })
}

/// Let go of domains seen at most once as a sender (link-only domains have
/// no count) and first seen more than [`PRUNE_AFTER`] before `now`: the
/// first-seen time and the count both go. Returns how many went.
pub fn prune(conn: &mut kevy_client::Connection, now: i64) -> io::Result<u64> {
    let mut pruned = 0u64;
    for (k, first) in all(conn)? {
        let Some(first) = first else { continue };
        if now - first <= PRUNE_AFTER {
            continue;
        }
        let reg = String::from_utf8_lossy(&k[PREFIX.len()..]).to_string();
        let seen_key = mailrs_fraud::brand::seen_key(&reg);
        let seen = number(conn.get(seen_key.as_bytes())?).unwrap_or(0);
        if seen > 1 {
            continue;
        }
        conn.del(&[k.as_slice(), seen_key.as_bytes()])?;
        pruned += 1;
    }
    Ok(pruned)
}

#[cfg(test)]
#[path = "domain_first_tests.rs"]
mod tests;

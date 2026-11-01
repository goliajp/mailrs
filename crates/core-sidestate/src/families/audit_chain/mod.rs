//! The audit log as a hash chain: a row changed, removed or added after it
//! was sealed is found by recomputing the chain, and a chain rewritten
//! wholesale is found against the replica kept outside the store.
//!
//! Writers are untouched: every process still appends facts with an id
//! from the shared counter. One process seals them, in id order, so the
//! chain needs no lock: the embedded store has no MULTI/WATCH and the
//! client no `SET NX`. The cost is a window, from a row's append to its
//! sealing (one tick), in which a change to it is not seen.
//!
//! A row's hash is SHA-256 over the previous row's hash and the row's
//! fields, each length-prefixed, in a fixed order. An id that never got a
//! row (a writer that took an id and died before appending it) is sealed
//! as a gap after [`GAP_WAIT`], so the chain does not stop there for good.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use kevy_client::Connection;
use sha2::{Digest, Sha256};

use super::audit::AuditFact;
use crate::keys::admin::{
    AUDIT_CHAIN, AUDIT_CHAIN_ANCHOR, AUDIT_CHAIN_HEAD, AUDIT_LOG, AUDIT_LOG_COUNTER,
};

mod replica;
mod run;
mod verify;

pub use replica::{compare as compare_replica, write as write_replica};
pub use run::{Status, daily, read_status, spawn};
pub use verify::{Report, prune, verify};

/// The hash before the first sealed row.
pub const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// How long a missing id is waited for before it is sealed as a gap.
pub const GAP_WAIT: Duration = Duration::from_secs(60);

/// Rows read past the expected ones on each tick: appends from several
/// processes land in the list slightly out of id order.
const SLACK: i64 = 256;

fn feed(h: &mut Sha256, part: &[u8]) {
    h.update((part.len() as u64).to_be_bytes());
    h.update(part);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The hash of `fact` sealed after `prev`.
pub fn row_hash(prev: &str, fact: &AuditFact) -> String {
    let mut h = Sha256::new();
    feed(&mut h, prev.as_bytes());
    feed(&mut h, fact.id.to_string().as_bytes());
    feed(&mut h, fact.occurred_at.to_string().as_bytes());
    feed(&mut h, fact.recorded_at.to_string().as_bytes());
    feed(&mut h, fact.actor.as_bytes());
    feed(&mut h, fact.action.as_bytes());
    feed(&mut h, fact.target.as_bytes());
    feed(&mut h, fact.detail.as_bytes());
    hex(&h.finalize())
}

/// The hash sealed for an id that has no row.
pub fn gap_hash(prev: &str, id: i64) -> String {
    let mut h = Sha256::new();
    feed(&mut h, prev.as_bytes());
    feed(&mut h, b"missing");
    feed(&mut h, id.to_string().as_bytes());
    hex(&h.finalize())
}

/// A sealed position: a row id and a hash (the row's own for the head,
/// the one sealed before it for the anchor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub id: i64,
    pub hash: String,
}

fn read_link(c: &mut Connection, key: &[u8]) -> io::Result<Option<Link>> {
    let flat = c.hgetall(key)?;
    let mut id = None;
    let mut hash = None;
    for pair in flat.chunks_exact(2) {
        let value = String::from_utf8_lossy(&pair[1]).into_owned();
        match pair[0].as_slice() {
            b"id" => id = value.parse().ok(),
            b"hash" => hash = Some(value),
            _ => {}
        }
    }
    Ok(id.zip(hash).map(|(id, hash)| Link { id, hash }))
}

fn write_link(c: &mut Connection, key: &[u8], l: &Link) -> io::Result<()> {
    c.hset(
        key,
        &[
            (b"id" as &[u8], l.id.to_string().as_bytes()),
            (b"hash", l.hash.as_bytes()),
        ],
    )?;
    Ok(())
}

/// The last sealed row.
pub fn head(c: &mut Connection) -> io::Result<Option<Link>> {
    read_link(c, AUDIT_CHAIN_HEAD)
}

/// The oldest sealed row kept, with the hash sealed before it.
pub fn anchor(c: &mut Connection) -> io::Result<Option<Link>> {
    read_link(c, AUDIT_CHAIN_ANCHOR)
}

pub(crate) fn set_anchor(c: &mut Connection, a: &Link) -> io::Result<()> {
    write_link(c, AUDIT_CHAIN_ANCHOR, a)
}

/// Every sealed hash, by row id.
pub fn chain(c: &mut Connection) -> io::Result<HashMap<i64, String>> {
    let flat = c.hgetall(AUDIT_CHAIN)?;
    Ok(flat
        .chunks_exact(2)
        .filter_map(|p| {
            let id = String::from_utf8_lossy(&p[0]).parse().ok()?;
            Some((id, String::from_utf8_lossy(&p[1]).into_owned()))
        })
        .collect())
}

/// The newest `n` rows of the log (all of them when `n` is `None`), and
/// how many did not parse.
pub(crate) fn rows(c: &mut Connection, n: Option<i64>) -> io::Result<(Vec<AuditFact>, usize)> {
    let stop = n.map_or(-1, |n| n - 1);
    let raw = c.lrange(AUDIT_LOG, 0, stop)?;
    let mut out = Vec::with_capacity(raw.len());
    let mut bad = 0;
    for r in raw {
        match serde_json::from_slice::<AuditFact>(&r) {
            Ok(f) => out.push(f),
            Err(_) => bad += 1,
        }
    }
    Ok((out, bad))
}

/// The sealing side. One per deployment; it keeps the gap it is waiting on.
#[derive(Debug, Default)]
pub struct Sealer {
    waiting: Option<(i64, Instant)>,
}

impl Sealer {
    /// Seal every row appended since the last tick, in id order. Reads the
    /// counter and the head when there is nothing new, and writes nothing.
    pub fn tick(&mut self, c: &mut Connection, now: Instant) -> io::Result<u64> {
        let counter: i64 = c
            .get(AUDIT_LOG_COUNTER)?
            .and_then(|v| String::from_utf8_lossy(&v).parse().ok())
            .unwrap_or(0);
        let last = head(c)?;
        if last.as_ref().is_some_and(|h| counter <= h.id) {
            return Ok(0);
        }
        let window = last.as_ref().map(|h| counter - h.id + SLACK);
        let (facts, _) = rows(c, window)?;
        let after = last.as_ref().map_or(0, |h| h.id);
        let by_id: HashMap<i64, AuditFact> = facts
            .into_iter()
            .filter(|f| f.id > after)
            .map(|f| (f.id, f))
            .collect();
        let (mut id, mut prev) = match &last {
            Some(h) => (h.id + 1, h.hash.clone()),
            None => {
                // the first seal starts at the oldest row there is
                let Some(&first) = by_id.keys().min() else {
                    return Ok(0);
                };
                let start = Link {
                    id: first,
                    hash: ZERO.into(),
                };
                set_anchor(c, &start)?;
                (first, ZERO.to_string())
            }
        };
        let mut sealed: Vec<(String, String)> = Vec::new();
        while id <= counter {
            let hash = match (by_id.get(&id), self.waiting) {
                (Some(f), _) => row_hash(&prev, f),
                (None, Some((gap, since))) if gap == id => {
                    if now.duration_since(since) < GAP_WAIT {
                        break;
                    }
                    tracing::warn!(id, "audit chain: an id was never appended, sealed as a gap");
                    gap_hash(&prev, id)
                }
                (None, _) => {
                    self.waiting = Some((id, now));
                    break;
                }
            };
            sealed.push((id.to_string(), hash.clone()));
            prev = hash;
            id += 1;
        }
        if sealed.is_empty() {
            return Ok(0);
        }
        if self.waiting.is_some_and(|(gap, _)| gap < id) {
            self.waiting = None;
        }
        let pairs: Vec<(&[u8], &[u8])> = sealed
            .iter()
            .map(|(k, v)| (k.as_bytes(), v.as_bytes()))
            .collect();
        c.hset(AUDIT_CHAIN, &pairs)?;
        let new_head = Link {
            id: id - 1,
            hash: prev,
        };
        write_link(c, AUDIT_CHAIN_HEAD, &new_head)?;
        Ok(sealed.len() as u64)
    }
}

#[cfg(test)]
mod tests;

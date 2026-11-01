//! Recomputing the chain over the rows the log still holds.
//!
//! A sealed row whose content changed hashes differently; a sealed row
//! that is gone hashes as a gap, which is not what was sealed for it; a
//! row added with an id already used, or older than the chain, is
//! reported as such. The oldest rows leave the log by design once it is
//! full (`audit::CAP`): absence below the oldest row kept is accepted only
//! when the log is at least that long.

use std::collections::HashMap;
use std::io;

use kevy_client::Connection;

use super::{Link, anchor, chain, gap_hash, head, row_hash, rows, set_anchor};
use crate::keys::admin::AUDIT_CHAIN;

/// What a verification found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub ok: bool,
    /// Sealed rows checked.
    pub rows: u64,
    /// The first row id found wrong, when one is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_bad_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The last sealed row id; `None` before anything was sealed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_id: Option<i64>,
}

fn fail(rows: u64, id: Option<i64>, reason: &str, head_id: Option<i64>) -> Report {
    Report {
        ok: false,
        rows,
        first_bad_id: id,
        reason: Some(reason.to_string()),
        head_id,
    }
}

/// Check the chain from its anchor to its head. Read-only.
pub fn verify(c: &mut Connection, cap: i64) -> io::Result<Report> {
    let Some(last) = head(c)? else {
        return Ok(Report {
            ok: true,
            rows: 0,
            first_bad_id: None,
            reason: None,
            head_id: None,
        });
    };
    let at = Some(last.id);
    let Some(start) = anchor(c)? else {
        return Ok(fail(0, None, "the chain has a head but no anchor", at));
    };
    let (facts, unreadable) = rows(c, None)?;
    if unreadable > 0 {
        let why = format!("{unreadable} rows of the log do not parse");
        return Ok(fail(0, None, &why, at));
    }
    let len = facts.len() as i64;
    let mut by_id = HashMap::with_capacity(facts.len());
    for f in facts {
        let id = f.id;
        if by_id.insert(id, f).is_some() {
            return Ok(fail(0, Some(id), "two rows have this id", at));
        }
    }
    let sealed = chain(c)?;
    let oldest = by_id.keys().min().copied();
    let trimmed = len >= cap && oldest.is_some_and(|o| o > start.id);
    let (first, mut prev) = match (trimmed, oldest) {
        (true, Some(o)) => match sealed.get(&(o - 1)) {
            Some(h) => (o, h.clone()),
            None => {
                let why = "the hash sealed before the oldest row kept is gone";
                return Ok(fail(0, Some(o), why, at));
            }
        },
        _ => (start.id, start.hash.clone()),
    };
    if let Some(&older) = by_id.keys().filter(|&&id| id < first).min() {
        return Ok(fail(0, Some(older), "a row is older than the chain", at));
    }
    let mut checked = 0u64;
    for id in first..=last.id {
        let Some(expected) = sealed.get(&id) else {
            return Ok(fail(
                checked,
                Some(id),
                "no hash was sealed for this row",
                at,
            ));
        };
        let (computed, why) = match by_id.get(&id) {
            Some(f) => (
                row_hash(&prev, f),
                "the row was changed after it was sealed",
            ),
            None => (
                gap_hash(&prev, id),
                "the row was removed after it was sealed",
            ),
        };
        if &computed != expected {
            return Ok(fail(checked, Some(id), why, at));
        }
        prev = expected.clone();
        checked += 1;
    }
    if last.hash != prev {
        let why = "the head does not match the chain";
        return Ok(fail(checked, Some(last.id), why, at));
    }
    Ok(Report {
        ok: true,
        rows: checked,
        first_bad_id: None,
        reason: None,
        head_id: at,
    })
}

/// Move the anchor up to the oldest row the log still holds, and drop the
/// sealed hashes before it. Run after a verification that passed.
/// Returns how many hashes were dropped.
pub fn prune(c: &mut Connection, cap: i64) -> io::Result<usize> {
    let Some(start) = anchor(c)? else {
        return Ok(0);
    };
    let (facts, _) = rows(c, None)?;
    if (facts.len() as i64) < cap {
        return Ok(0);
    }
    let Some(oldest) = facts.iter().map(|f| f.id).min() else {
        return Ok(0);
    };
    if oldest <= start.id {
        return Ok(0);
    }
    let sealed = chain(c)?;
    let Some(before) = sealed.get(&(oldest - 1)) else {
        return Ok(0);
    };
    set_anchor(
        c,
        &Link {
            id: oldest,
            hash: before.clone(),
        },
    )?;
    let doomed: Vec<String> = sealed
        .keys()
        .filter(|&&id| id < oldest)
        .map(|id| id.to_string())
        .collect();
    let refs: Vec<&[u8]> = doomed.iter().map(|s| s.as_bytes()).collect();
    if !refs.is_empty() {
        c.hdel(AUDIT_CHAIN, &refs)?;
    }
    Ok(doomed.len())
}

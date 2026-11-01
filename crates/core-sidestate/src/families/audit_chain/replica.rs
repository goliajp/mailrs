//! The copy of the chain kept outside the store.
//!
//! Each run writes the rows sealed since the last one into
//! `audit-<first>-<last>.jsonl`: one line per row (the fact and its hash,
//! or the gap and its hash) and a last line with the head. The file is
//! written under a temporary name and renamed, and its `.sha256` is
//! written last, the convention the backups follow: whoever copies the
//! directory off the server takes only files whose checksum exists.
//!
//! Someone holding the files can recompute the chain without the store,
//! and the daily check compares every sealed hash the store still has
//! with the replica's: a chain rewritten in the store, rows and hashes
//! alike, is found here.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use kevy_client::Connection;
use sha2::{Digest, Sha256};

use super::{anchor, chain, head, hex, rows};
use crate::keys::admin::AUDIT_CHAIN_REPLICATED;

/// The last row id written to the replica.
pub fn replicated_through(c: &mut Connection) -> io::Result<i64> {
    Ok(c.get(AUDIT_CHAIN_REPLICATED)?
        .and_then(|v| String::from_utf8_lossy(&v).parse().ok())
        .unwrap_or(0))
}

fn write_atomically(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, dir.join(name))
}

/// Write the rows sealed since the last run. Returns the ids written, or
/// `None` when there was nothing new.
pub fn write(dir: &Path, c: &mut Connection) -> io::Result<Option<(i64, i64)>> {
    let Some(last) = head(c)? else {
        return Ok(None);
    };
    let done = replicated_through(c)?;
    if last.id <= done {
        return Ok(None);
    }
    let start = anchor(c)?.map_or(done + 1, |a| a.id.max(done + 1));
    let sealed = chain(c)?;
    let (facts, _) = rows(c, None)?;
    let by_id: HashMap<i64, _> = facts.into_iter().map(|f| (f.id, f)).collect();
    let mut body = Vec::new();
    for id in start..=last.id {
        let Some(hash) = sealed.get(&id) else {
            continue;
        };
        let line = match by_id.get(&id) {
            Some(f) => serde_json::json!({ "id": id, "fact": f, "hash": hash }),
            None => serde_json::json!({ "id": id, "missing": true, "hash": hash }),
        };
        body.extend_from_slice(line.to_string().as_bytes());
        body.push(b'\n');
    }
    let tail = serde_json::json!({ "head": { "id": last.id, "hash": last.hash } });
    body.extend_from_slice(tail.to_string().as_bytes());
    body.push(b'\n');
    let name = format!("audit-{start}-{}.jsonl", last.id);
    write_atomically(dir, &name, &body)?;
    let sum = format!("{}  {name}\n", hex(&Sha256::digest(&body)));
    write_atomically(dir, &format!("{name}.sha256"), sum.as_bytes())?;
    c.set(AUDIT_CHAIN_REPLICATED, last.id.to_string().as_bytes())?;
    Ok(Some((start, last.id)))
}

/// Compare the store's sealed hashes with the replica's. `Ok(n)` with how
/// many were compared, or the first disagreement: the row id (when it is
/// one row) and why.
pub fn compare(dir: &Path, c: &mut Connection) -> io::Result<Result<usize, (Option<i64>, String)>> {
    let mut copied: HashMap<i64, String> = HashMap::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !(name.starts_with("audit-") && name.ends_with(".jsonl")) {
            continue;
        }
        // a file without its checksum was not finished
        let Ok(sum) = fs::read_to_string(dir.join(format!("{name}.sha256"))) else {
            continue;
        };
        let body = fs::read(&path)?;
        if sum.split_whitespace().next() != Some(hex(&Sha256::digest(&body)).as_str()) {
            return Ok(Err((None, format!("{name} does not match its checksum"))));
        }
        for line in body.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
                return Ok(Err((
                    None,
                    format!("{name} has a line that does not parse"),
                )));
            };
            if let (Some(id), Some(hash)) = (v["id"].as_i64(), v["hash"].as_str()) {
                copied.insert(id, hash.to_string());
            }
        }
    }
    let sealed = chain(c)?;
    let mut ids: Vec<&i64> = copied.keys().collect();
    ids.sort();
    let mut compared = 0;
    for id in ids {
        let Some(here) = sealed.get(id) else {
            continue;
        };
        if here != &copied[id] {
            let why = "the store's sealed hash differs from the replica's";
            return Ok(Err((Some(*id), why.to_string())));
        }
        compared += 1;
    }
    Ok(Ok(compared))
}

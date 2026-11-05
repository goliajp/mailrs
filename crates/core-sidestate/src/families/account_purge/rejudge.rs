//! A re-judge round's gathered conversations in fastcore's store
//! (`rejudge:candidates:{obs}`, fields `user\ttid`). A purged reader's go.
//! When the round waits as a pending batch (`rejudge:batches`, field
//! `obs`), its `would_hold` drops by as many; a batch left with none, or
//! with no gathered conversation at all, is dropped: there is nothing to
//! decide, and confirming or discarding it answers 404, as for any batch
//! that is not there.

use std::io;

use kevy_client::Connection;

use super::shape::scan_all;

const CANDIDATES: &str = "rejudge:candidates:";
const BATCHES: &[u8] = b"rejudge:batches";

/// Whether field `f` (`user\ttid`) is `address`'s.
fn is_theirs(f: &[u8], address: &str) -> bool {
    let a = address.as_bytes();
    f.len() > a.len() && f[..a.len()].eq_ignore_ascii_case(a) && f[a.len()] == b'\t'
}

/// Count `address`'s gathered conversations, or remove them and lower the
/// pending batches they were counted in.
pub(super) fn candidates(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    let mut n = 0;
    for key in scan_all(c, &format!("{CANDIDATES}*"))? {
        let theirs: Vec<Vec<u8>> = c
            .hkeys(&key)?
            .into_iter()
            .filter(|f| is_theirs(f, address))
            .collect();
        if theirs.is_empty() {
            continue;
        }
        n += theirs.len() as u64;
        if dry_run {
            continue;
        }
        let refs: Vec<&[u8]> = theirs.iter().map(Vec::as_slice).collect();
        c.hdel(&key, &refs)?;
        let left = c.hlen(&key)?;
        lower(c, &key[CANDIDATES.len()..], theirs.len() as u64, left == 0)?;
    }
    Ok(n)
}

/// Take `removed` off pending batch `obs`, dropping it at none, or when
/// `none_left` of its gathered conversations remain.
fn lower(c: &mut Connection, obs: &[u8], removed: u64, none_left: bool) -> io::Result<()> {
    let Some(raw) = c.hget(BATCHES, obs)? else {
        return Ok(());
    };
    let Ok(mut batch) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return Ok(());
    };
    if batch.get("pending").and_then(|p| p.as_bool()) != Some(true) {
        return Ok(());
    }
    let would_hold = batch
        .get("would_hold")
        .and_then(|w| w.as_u64())
        .unwrap_or(0);
    let left = would_hold.saturating_sub(removed);
    if left == 0 || none_left {
        c.hdel(BATCHES, &[obs])?;
        return Ok(());
    }
    batch["would_hold"] = left.into();
    let json = serde_json::to_vec(&batch).map_err(io::Error::other)?;
    c.hset(BATCHES, &[(obs, json.as_slice())])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_field_is_its_readers_in_any_spelling_and_no_one_elses() {
        assert!(super::is_theirs(b"Bob@x.com\tt1", "bob@x.com"));
        assert!(!super::is_theirs(b"bob@x.comm\tt1", "bob@x.com"));
        assert!(!super::is_theirs(b"bob@x.com", "bob@x.com"));
    }
}

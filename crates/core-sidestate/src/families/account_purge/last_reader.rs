//! Rows fastcore's store keeps once per message rather than once per
//! reader: a message's template fingerprint (`L:t:{mid}`) and its segment
//! row (`L:ts:{mid}`, while the message is findable). They go with the
//! message's last reader. A message's readers are its ledger rows
//! (`L:m:{user}|{mid}`), found through the ledger's `mid` index, so these
//! entries run before the one that removes the account's ledger rows.

use std::io;

use kevy_client::Connection;
use kevy_embedded::IndexValue;

use super::shape::{glob_escape, scan_all};

const LEDGER_PREFIX: &str = "L:m:";
const LEDGER_BY_MID: &[u8] = b"ledger_msg.mid";
/// More readers than one message has.
const READERS_CAP: usize = 10_000;

/// Fingerprints of the messages `address` is the last reader of.
pub(super) fn fingerprints(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    of_last_reader(c, address, dry_run, "L:t:")
}

/// Segment rows of the messages `address` is the last reader of.
pub(super) fn segments(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    of_last_reader(c, address, dry_run, "L:ts:")
}

/// The message ids `address` (this spelling) has ledger rows for. Fastcore
/// reads them before deleting the account's conversations, which takes the
/// ledger rows with them, to remove the fingerprints of the messages it
/// was the last reader of after.
pub fn messages_read_by(c: &mut Connection, address: &str) -> io::Result<Vec<Vec<u8>>> {
    let own = format!("{LEDGER_PREFIX}{}|*", glob_escape(address));
    let mut mids = Vec::new();
    for key in scan_all(c, &own)? {
        if let Some(mid) = c.hget(&key, b"mid")? {
            mids.push(mid);
        }
    }
    Ok(mids)
}

fn of_last_reader(
    c: &mut Connection,
    address: &str,
    dry_run: bool,
    prefix: &str,
) -> io::Result<u64> {
    let mids = messages_read_by(c, address)?;
    // only fastcore's store has the ledger; the SQL core keeps its own
    let Connection::Embedded(store) = c else {
        return Ok(0);
    };
    let mut n = 0;
    for mid in mids {
        let v = IndexValue::Str(mid.clone());
        let (readers, _) = store.idx_query(LEDGER_BY_MID, &v, &v, None, READERS_CAP)?;
        if readers.iter().any(|(k, _)| !read_by(k, &mid, address)) {
            continue;
        }
        let key = [prefix.as_bytes(), mid.as_slice()].concat();
        if store.exists(&[key.as_slice()])? == 0 {
            continue;
        }
        n += 1;
        if !dry_run {
            store.del(&[key.as_slice()])?;
        }
    }
    Ok(n)
}

/// Whether ledger row `key` (`L:m:{user}|{mid}`) is `address`'s, in any
/// spelling.
fn read_by(key: &[u8], mid: &[u8], address: &str) -> bool {
    let user = key
        .strip_prefix(LEDGER_PREFIX.as_bytes())
        .and_then(|k| k.strip_suffix(mid))
        .and_then(|k| k.strip_suffix(b"|"));
    user.is_some_and(|u| u.eq_ignore_ascii_case(address.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ledger_row_is_read_by_its_user_in_any_spelling() {
        let key = b"L:m:Bob@X.com|m|1@y";
        assert!(read_by(key, b"m|1@y", "bob@x.com"));
        assert!(!read_by(key, b"m|1@y", "alice@x.com"));
        assert!(!read_by(b"L:m:bob@x.com|other", b"m|1@y", "bob@x.com"));
    }
}

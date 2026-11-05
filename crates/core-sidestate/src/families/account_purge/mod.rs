//! Purging a deleted account: everything the address still owns, after the
//! retention period.
//!
//! The delete (either core's) parks the maildir ([`maildir::park`]), drops
//! the account record and marks the address in [`pending`]. Once
//! [`pending::retention_days`] have passed, the purge removes the parked
//! maildir, the core's mail rows, and every family [`registry::OWNED`]
//! lists, then writes one audit row with what it removed and clears the
//! mark. The pieces here are what both cores share; each core removes its
//! own mail rows (fastcore conversation by conversation, so records other
//! owners share stay; the SQL core by [`registry::SQL_TABLES`]).
//!
//! Until the purge runs, [`restore`] undoes the delete: the core puts its
//! account record back (fastcore from the record [`pending::keep_record`]
//! kept, the SQL core by clearing its row's `deleted_at`) and the parked
//! maildir goes back where delivery finds it.

mod custom;
pub mod last_reader;
pub mod maildir;
pub mod pending;
pub mod registry;
pub mod restore;
mod shape;
mod sql_tables;

use std::io;

use kevy_client::Connection;
use mailrs_core_api::method::admin::{AccountPurgeRow, PurgeCount};

use registry::{KEPT, OWNED, Store};
pub use shape::{Shape, glob_escape, scan_all};

/// The audit action of a purge.
pub const PURGED: &str = "account.purged";

/// What [`due`] found waiting.
#[derive(Debug, Default)]
pub struct Due {
    /// Addresses waiting, due or not.
    pub pending: usize,
    /// Those whose retention has passed (or the one asked for, when it
    /// has), with the time of their delete.
    pub due: Vec<(String, i64)>,
}

/// The pending addresses whose `retention_days` have passed at `now`;
/// only `only` when given.
pub fn due(
    c: &mut Connection,
    now: i64,
    retention_days: u64,
    only: Option<&str>,
) -> io::Result<Due> {
    let all = pending::all(c)?;
    let cutoff = now - (retention_days as i64).saturating_mul(86_400);
    let due = all
        .iter()
        .filter(|(a, at)| *at <= cutoff && only.is_none_or(|o| o == a))
        .cloned()
        .collect();
    Ok(Due {
        pending: all.len(),
        due,
    })
}

/// Count `store`'s registered entries for `address`, each spelling of it.
pub fn count_store(c: &mut Connection, store: Store, address: &str) -> io::Result<Vec<PurgeCount>> {
    walk(c, store, address, true)
}

/// Remove `store`'s registered entries for `address`.
pub fn remove_store(c: &mut Connection, store: Store, address: &str) -> io::Result<()> {
    walk(c, store, address, false).map(|_| ())
}

fn walk(
    c: &mut Connection,
    store: Store,
    address: &str,
    dry_run: bool,
) -> io::Result<Vec<PurgeCount>> {
    let mut counts: Vec<PurgeCount> = Vec::new();
    for owned in OWNED.iter().filter(|o| o.store == store) {
        let mut n = 0;
        for spelling in registry::spellings(address) {
            n += owned.shape.apply(c, &spelling, dry_run)?;
        }
        match counts.iter_mut().find(|p| p.name == owned.name) {
            Some(p) => p.count += n,
            None => counts.push(PurgeCount {
                name: owned.name.to_string(),
                count: n,
            }),
        }
    }
    counts.retain(|p| p.count > 0);
    Ok(counts)
}

/// A registry name as a glob over keys, for `address` (escaped): `{user}`
/// is the address, `*` and `{gid}` anything, and a note after " (" is not
/// part of the key.
fn name_glob(name: &str, address: &str) -> String {
    let name = name.split(" (").next().unwrap_or(name);
    name.replace("{gid}", "*")
        .replace("{user}", &glob_escape(address))
}

/// Keys of `c` that carry `address` in their name and that no registry
/// entry, owned or kept, names: by the part of the key before the address.
/// On a dry run, families the registry misses; after a purge, also what it
/// failed to remove.
pub fn unregistered(c: &mut Connection, address: &str) -> io::Result<Vec<PurgeCount>> {
    let mut out: Vec<PurgeCount> = Vec::new();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    for spelling in registry::spellings(address) {
        let named: Vec<String> = OWNED
            .iter()
            .map(|o| o.name)
            .chain(KEPT.iter().map(|k| k.name))
            .map(|n| name_glob(n, &spelling))
            .collect();
        for key in scan_all(c, &format!("*{}*", glob_escape(&spelling)))? {
            if seen.contains(&key) {
                continue;
            }
            let name = String::from_utf8_lossy(&key).into_owned();
            if named.iter().any(|g| glob_match(g, &name)) {
                continue;
            }
            seen.push(key);
            let at = name.find(spelling.as_str()).unwrap_or(0);
            let prefix = format!("{}{{user}}", &name[..at]);
            match out.iter_mut().find(|p| p.name == prefix) {
                Some(p) => p.count += 1,
                None => out.push(PurgeCount {
                    name: prefix,
                    count: 1,
                }),
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// `name` against a glob of `*` and `\`-escaped characters.
fn glob_match(glob: &str, name: &str) -> bool {
    let mut parts: Vec<Option<char>> = Vec::new();
    let mut chars = glob.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '*' => parts.push(None),
            '\\' => parts.push(chars.next()),
            other => parts.push(Some(other)),
        }
    }
    let name: Vec<char> = name.chars().collect();
    // star positions match greedily with backtracking to the last star
    let (mut p, mut n) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match parts.get(p) {
            Some(None) => {
                star = Some((p, n));
                p += 1;
            }
            Some(Some(ch)) if *ch == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    p = sp + 1;
                    n = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    parts[p..].iter().all(Option::is_none)
}

/// Write the purge's audit row and stop waiting for the address. Last, so
/// a purge that failed part-way is retried by the next pass.
pub fn finish(c: &mut Connection, row: &AccountPurgeRow, now: i64) -> io::Result<()> {
    let detail = serde_json::json!({
        "deleted_at": row.deleted_at,
        "maildir_files": row.maildir_files,
        "maildir_bytes": row.maildir_bytes,
        "mail_store": row.mail_store,
        "network": row.network,
        "unregistered": row.unregistered,
    })
    .to_string();
    crate::families::audit::append(c, "system", PURGED, &row.address, &detail, now)?;
    pending::clear(c, &row.address)
}

#[cfg(test)]
mod tests;

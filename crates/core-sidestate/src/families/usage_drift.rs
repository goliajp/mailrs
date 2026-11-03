//! The quota usage counter (`mailrs:quota:<user>:used_bytes`, what the
//! receiver's quota stage reads) against what each account's maildir takes
//! on disk, and the pass that sets the counter to the disk figure. Both
//! cores run it; both keep the counter in the shared store and the mail in
//! the same maildir layout.
//!
//! The counter is a derivation of the files: arrivals add, removals
//! subtract, and this pass recomputes it from the files themselves. A
//! measurement takes time and mail keeps arriving, so a counter is written
//! only when it still holds the value read before its maildir was
//! measured; one that moved is left for the next pass.

use std::io;
use std::path::{Path, PathBuf};

use kevy_client::Connection;
use mailrs_core_api::method::admin::{
    OrphanCounter, UsageDriftAction, UsageDriftResponse, UsageDriftRow,
};

use crate::families::audit;
use crate::keys::quota::used_key;

/// The audit action of one counter this pass corrected.
pub const USAGE_CORRECTED: &str = "quota.usage_corrected";

fn dir_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(iter) = std::fs::read_dir(dir) {
        for e in iter.flatten() {
            if let Ok(md) = e.metadata()
                && md.is_file()
            {
                total += md.len();
            }
        }
    }
    total
}

/// What the mail of the account at `base` takes: `cur/` and `new/` of its
/// INBOX and of every Maildir++ subfolder. `tmp/` holds mail not yet
/// delivered, which the counter has not counted either.
pub fn account_bytes(base: &Path) -> u64 {
    let mut total = dir_bytes(&base.join("cur")) + dir_bytes(&base.join("new"));
    if let Ok(iter) = std::fs::read_dir(base) {
        for e in iter.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') || name == "." || name == ".." {
                continue;
            }
            let sub = e.path();
            if sub.join("new").is_dir() {
                total += dir_bytes(&sub.join("cur")) + dir_bytes(&sub.join("new"));
            }
        }
    }
    total
}

/// `(account, directory)` for every account maildir under `root`, sorted.
/// Dot-directories at the domain level are not domains: `.deleted` holds
/// deleted accounts' mail.
///
/// # Errors
/// `root` could not be read.
pub fn account_dirs(root: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for d in std::fs::read_dir(root)?.flatten() {
        if !d.file_type().is_ok_and(|t| t.is_dir())
            || d.file_name().to_string_lossy().starts_with('.')
        {
            continue;
        }
        let domain = d.file_name().to_string_lossy().into_owned();
        let Ok(locals) = std::fs::read_dir(d.path()) else {
            continue;
        };
        for l in locals.flatten() {
            if !l.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let local = l.file_name().to_string_lossy().into_owned();
            out.push((format!("{local}@{domain}").to_lowercase(), l.path()));
        }
    }
    out.sort();
    Ok(out)
}

/// `(account, bytes)` for every account maildir under `root`, sorted.
///
/// # Errors
/// `root` could not be read.
pub fn maildir_usage(root: &Path) -> io::Result<Vec<(String, u64)>> {
    Ok(account_dirs(root)?
        .into_iter()
        .map(|(a, dir)| {
            let bytes = account_bytes(&dir);
            (a, bytes)
        })
        .collect())
}

/// Accounts with a usage counter in the shared store, maildir or not.
fn counted_accounts(conn: &mut Connection) -> io::Result<Vec<String>> {
    let prefix = used_key("");
    let (head, tail) = prefix
        .split_once("::")
        .map(|(h, t)| (format!("{h}:"), format!(":{t}")))
        .unwrap_or_default();
    let pattern = format!("{head}*{tail}");
    let mut out = Vec::new();
    let mut cursor = 0u64;
    loop {
        let (next, keys) = conn.scan(cursor, Some(pattern.as_bytes()), None)?;
        for k in keys {
            let k = String::from_utf8_lossy(&k).into_owned();
            if let Some(user) = k.strip_prefix(&head).and_then(|r| r.strip_suffix(&tail)) {
                out.push(user.to_string());
            }
        }
        if next == 0 {
            return Ok(out);
        }
        cursor = next;
    }
}

fn read_counter(conn: &mut Connection, key: &str) -> io::Result<Option<i64>> {
    Ok(conn
        .get(key.as_bytes())?
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.trim().parse::<i64>().ok()))
}

/// Read the counter as the pass will compare it. On the shared server the
/// key is watched first, so a write between here and [`set_if_unchanged`]
/// aborts that write.
fn begin(conn: &mut Connection, key: &str) -> io::Result<Option<i64>> {
    if let Connection::Remote(_) = conn {
        conn.watch(&[key.as_bytes()])?;
    }
    read_counter(conn, key)
}

/// Drop the watch [`begin`] set, for an account the pass does not write.
fn end(conn: &mut Connection) -> io::Result<()> {
    if let Connection::Remote(_) = conn {
        conn.unwatch()?;
    }
    Ok(())
}

/// Set `key` to `value` only if it still holds `before`. True when it
/// wrote. On the shared server this is the watched transaction; an
/// in-process store (tests, a core without a shared server) compares and
/// writes under no lock between the two, which nothing else contends for.
fn set_if_unchanged(
    conn: &mut Connection,
    key: &str,
    before: Option<i64>,
    value: i64,
) -> io::Result<bool> {
    let text = value.to_string();
    match conn {
        Connection::Remote(_) => {
            let mut txn = conn.multi()?;
            txn.set(key.as_bytes(), text.as_bytes())?;
            Ok(txn.exec_watched()?.is_some())
        }
        Connection::Embedded(_) => {
            if read_counter(conn, key)? != before {
                return Ok(false);
            }
            conn.set(key.as_bytes(), text.as_bytes())?;
            Ok(true)
        }
    }
}

/// One account as the pass found it.
struct Step<'a> {
    account: &'a str,
    key: &'a str,
    before: Option<i64>,
    actual: u64,
    differs: bool,
    dry_run: bool,
    now: i64,
}

/// What to do with one counter, after [`begin`] watched and read it.
fn settle(conn: &mut Connection, s: &Step<'_>) -> io::Result<UsageDriftAction> {
    if !s.differs {
        end(conn)?;
        return Ok(UsageDriftAction::Equal);
    }
    if s.dry_run {
        end(conn)?;
        return Ok(UsageDriftAction::WouldWrite);
    }
    let value = i64::try_from(s.actual).unwrap_or(i64::MAX);
    if !set_if_unchanged(conn, s.key, s.before, value)? {
        return Ok(UsageDriftAction::SkippedRaced);
    }
    let before = s
        .before
        .map_or_else(|| "none".to_string(), |b| b.to_string());
    let detail = format!("before={before} written={value} actual={}", s.actual);
    audit::append(conn, "system", USAGE_CORRECTED, s.account, &detail, s.now)?;
    Ok(UsageDriftAction::Written)
}

/// Delete `key` only if it still holds `before`. True when it deleted.
fn delete_if_unchanged(conn: &mut Connection, key: &str, before: Option<i64>) -> io::Result<bool> {
    match conn {
        Connection::Remote(_) => {
            let mut txn = conn.multi()?;
            txn.queue(&[b"DEL", key.as_bytes()])?;
            Ok(txn.exec_watched()?.is_some())
        }
        Connection::Embedded(_) => {
            if read_counter(conn, key)? != before {
                return Ok(false);
            }
            conn.del(&[key.as_bytes()])?;
            Ok(true)
        }
    }
}

/// What to do with a counter whose address is not an account: delete it,
/// unless it moved since [`begin`] read it.
fn settle_orphan(
    conn: &mut Connection,
    address: &str,
    before: Option<i64>,
    dry_run: bool,
    now: i64,
) -> io::Result<UsageDriftAction> {
    if dry_run {
        end(conn)?;
        return Ok(UsageDriftAction::WouldWrite);
    }
    if !delete_if_unchanged(conn, &used_key(address), before)? {
        return Ok(UsageDriftAction::SkippedRaced);
    }
    let before = before.map_or_else(|| "none".to_string(), |b| b.to_string());
    let detail = format!("before={before} written=deleted");
    audit::append(conn, "system", USAGE_CORRECTED, address, &detail, now)?;
    Ok(UsageDriftAction::Written)
}

/// Measure each account's maildir and compare it with its counter; with
/// `dry_run` false, correct every counter that differs and did not move
/// meanwhile, and delete the counters of addresses that are not accounts,
/// one audit row each. `accounts` is the core's account list; a maildir
/// without an account is measured as before, a counter without an account
/// is an orphan. `root` is the maildir root; `now` the audit time.
///
/// # Errors
/// `root` or the shared store could not be read, or a write failed.
pub fn pass(
    conn: &mut Connection,
    root: &Path,
    accounts: &[String],
    dry_run: bool,
    now: i64,
) -> io::Result<UsageDriftResponse> {
    pass_measuring(conn, root, accounts, dry_run, now, account_bytes)
}

/// [`pass`] with the measurement given, so a test can make mail arrive
/// while an account is being measured.
pub fn pass_measuring(
    conn: &mut Connection,
    root: &Path,
    accounts: &[String],
    dry_run: bool,
    now: i64,
    mut measure: impl FnMut(&Path) -> u64,
) -> io::Result<UsageDriftResponse> {
    let known: std::collections::BTreeSet<String> =
        accounts.iter().map(|a| a.to_lowercase()).collect();
    let mut measured: Vec<(String, Option<PathBuf>)> = account_dirs(root)?
        .into_iter()
        .map(|(a, dir)| (a, Some(dir)))
        .collect();
    let mut orphans = Vec::new();
    for a in counted_accounts(conn)? {
        if measured.iter().any(|(have, _)| *have == a) {
            continue;
        }
        if known.contains(&a) {
            measured.push((a, None));
        } else {
            orphans.push(a);
        }
    }
    measured.sort();
    orphans.sort();

    let mut out = UsageDriftResponse {
        dry_run,
        accounts: measured.len(),
        differing: 0,
        counter_low: Vec::new(),
        written: 0,
        skipped_raced: 0,
        raced: Vec::new(),
        rows: Vec::with_capacity(measured.len()),
        orphan_counters: Vec::with_capacity(orphans.len()),
        orphans_deleted: 0,
    };
    for (account, dir) in measured {
        let key = used_key(&account);
        let before = begin(conn, &key)?;
        let actual = dir.as_deref().map_or(0, &mut measure);
        let actual_i = i64::try_from(actual).unwrap_or(i64::MAX);
        let differs = match before {
            Some(c) => c != actual_i,
            None => actual > 0,
        };
        let step = Step {
            account: &account,
            key: &key,
            before,
            actual,
            differs,
            dry_run,
            now,
        };
        let action = settle(conn, &step)?;
        match action {
            UsageDriftAction::Written => out.written += 1,
            UsageDriftAction::SkippedRaced => {
                out.skipped_raced += 1;
                out.raced.push(account.clone());
            }
            UsageDriftAction::Equal | UsageDriftAction::WouldWrite => {}
        }
        if differs {
            out.differing += 1;
        }
        let low = match before {
            Some(c) => c < actual_i,
            None => actual > 0,
        };
        if low {
            out.counter_low.push(account.clone());
        }
        out.rows.push(UsageDriftRow {
            account,
            counter: before,
            actual,
            diff: before.map(|c| c - actual_i),
            action,
        });
    }
    for address in orphans {
        let before = begin(conn, &used_key(&address))?;
        let action = settle_orphan(conn, &address, before, dry_run, now)?;
        match action {
            UsageDriftAction::Written => out.orphans_deleted += 1,
            UsageDriftAction::SkippedRaced => {
                out.skipped_raced += 1;
                out.raced.push(address.clone());
            }
            UsageDriftAction::Equal | UsageDriftAction::WouldWrite => {}
        }
        out.orphan_counters.push(OrphanCounter {
            address,
            counter: before,
            action,
        });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "usage_drift_tests.rs"]
mod tests;

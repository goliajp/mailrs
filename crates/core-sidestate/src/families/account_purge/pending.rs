//! Deleted accounts waiting for their purge.
//!
//! ```text
//! mailrs:purge:pending   hash   address -> unix seconds of the delete
//! mailrs:purge:records   hash   address -> the account record the delete
//!                               dropped, for a restore (fastcore's; the
//!                               SQL core keeps its row)
//! ```
//!
//! Written by the account delete of whichever core runs, read by its purge
//! and its restore. A field goes when the purge of its address has removed
//! everything, or a restore has brought it back, so the hash is empty when
//! nothing is waiting, and the retention task reads one key to learn that.

use std::io;

use kevy_client::Connection;

pub const PENDING_KEY: &str = "mailrs:purge:pending";
pub const RECORDS_KEY: &str = "mailrs:purge:records";

/// Days a deleted account's data is kept before the purge removes it:
/// `MAILRS_DELETED_ACCOUNT_DAYS`, 30 when unset; 0 purges at the delete.
pub const RETENTION_ENV: &str = "MAILRS_DELETED_ACCOUNT_DAYS";
pub const DEFAULT_RETENTION_DAYS: u64 = 30;

/// The retention period from `value` of [`RETENTION_ENV`]. A value that is
/// not a number keeps the default and says so: the safe way to misread a
/// setting that deletes data is to delete later.
pub fn retention_days_from(value: Option<&str>) -> u64 {
    let Some(v) = value else {
        return DEFAULT_RETENTION_DAYS;
    };
    match v.trim().parse() {
        Ok(days) => days,
        Err(_) => {
            tracing::error!(value = %v, "{RETENTION_ENV} is not a number of days; keeping {DEFAULT_RETENTION_DAYS}");
            DEFAULT_RETENTION_DAYS
        }
    }
}

/// [`retention_days_from`] the process environment.
pub fn retention_days() -> u64 {
    retention_days_from(std::env::var(RETENTION_ENV).ok().as_deref())
}

/// Record `address` as deleted at `now`. A second delete of an address
/// already waiting keeps the first time: its retention runs from then.
pub fn mark(c: &mut Connection, address: &str, now: i64) -> io::Result<()> {
    if c.hget(PENDING_KEY.as_bytes(), address.as_bytes())?
        .is_some()
    {
        return Ok(());
    }
    let at = now.to_string();
    c.hset(
        PENDING_KEY.as_bytes(),
        &[(address.as_bytes(), at.as_bytes())],
    )?;
    Ok(())
}

/// When `address` was deleted, if it is waiting for its purge.
pub fn deleted_at(c: &mut Connection, address: &str) -> io::Result<Option<i64>> {
    let v = c.hget(PENDING_KEY.as_bytes(), address.as_bytes())?;
    Ok(v.and_then(|b| String::from_utf8_lossy(&b).trim().parse().ok()))
}

/// Every address waiting, with the time of its delete, sorted by address.
pub fn all(c: &mut Connection) -> io::Result<Vec<(String, i64)>> {
    let flat = c.hgetall(PENDING_KEY.as_bytes())?;
    let mut out: Vec<(String, i64)> = flat
        .chunks(2)
        .filter_map(|p| {
            let addr = String::from_utf8(p.first()?.clone()).ok()?;
            let at = String::from_utf8_lossy(p.get(1)?).trim().parse().ok()?;
            Some((addr, at))
        })
        .collect();
    out.sort();
    Ok(out)
}

/// Stop waiting for `address`: its purge is done, or the delete that
/// marked it did not happen.
pub fn clear(c: &mut Connection, address: &str) -> io::Result<()> {
    c.hdel(RECORDS_KEY.as_bytes(), &[address.as_bytes()])?;
    c.hdel(PENDING_KEY.as_bytes(), &[address.as_bytes()])?;
    Ok(())
}

/// Keep `record`, the account record the delete of `address` drops, until
/// the purge or a restore.
pub fn keep_record(c: &mut Connection, address: &str, record: &str) -> io::Result<()> {
    c.hset(
        RECORDS_KEY.as_bytes(),
        &[(address.as_bytes(), record.as_bytes())],
    )?;
    Ok(())
}

/// The record [`keep_record`] kept for `address`.
pub fn record(c: &mut Connection, address: &str) -> io::Result<Option<String>> {
    let v = c.hget(RECORDS_KEY.as_bytes(), address.as_bytes())?;
    Ok(v.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retention_setting_reads_days_and_keeps_the_default_on_nonsense() {
        assert_eq!(retention_days_from(None), 30);
        assert_eq!(retention_days_from(Some("0")), 0);
        assert_eq!(retention_days_from(Some(" 7 ")), 7);
        assert_eq!(retention_days_from(Some("seven")), 30);
        assert_eq!(retention_days_from(Some("-1")), 30);
    }

    #[test]
    fn a_second_delete_keeps_the_first_time() {
        let mut c = Connection::connect("mem://purge-pending-mark").unwrap();
        mark(&mut c, "a@x.test", 100).unwrap();
        mark(&mut c, "a@x.test", 200).unwrap();
        mark(&mut c, "b@x.test", 150).unwrap();
        assert_eq!(deleted_at(&mut c, "a@x.test").unwrap(), Some(100));
        assert_eq!(
            all(&mut c).unwrap(),
            [("a@x.test".to_string(), 100), ("b@x.test".to_string(), 150)]
        );
        keep_record(&mut c, "a@x.test", "{}").unwrap();
        assert_eq!(record(&mut c, "a@x.test").unwrap().as_deref(), Some("{}"));
        clear(&mut c, "a@x.test").unwrap();
        assert_eq!(deleted_at(&mut c, "a@x.test").unwrap(), None);
        assert_eq!(
            record(&mut c, "a@x.test").unwrap(),
            None,
            "the record outlived the mark"
        );
    }
}

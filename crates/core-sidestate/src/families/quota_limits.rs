//! Per-account quota limits in the shared store: the default each new
//! account gets, the pass that gives it to accounts created without one,
//! and the census `/api/health` reports. The receiver enforces the limit
//! at RCPT time, so an account without the key has no quota at all.

use std::io;

use kevy_client::Connection;
use mailrs_core_api::method::admin::{
    DEFAULT_QUOTA_BYTES, NEAR_FULL_PERCENT, QuotaLimitAction, QuotaLimitRow, QuotaLimitsResponse,
};

use crate::keys::quota::{limit_key, used_key};

fn number(conn: &mut Connection, key: &str) -> io::Result<Option<i64>> {
    Ok(conn
        .get(key.as_bytes())?
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.trim().parse::<i64>().ok()))
}

/// The user's limit; `None` when there is none (a stored 0 means
/// unlimited, as the receiver reads it).
pub fn limit(conn: &mut Connection, user: &str) -> io::Result<Option<i64>> {
    Ok(number(conn, &limit_key(user))?.filter(|l| *l > 0))
}

/// The user's usage counter; 0 when there is none.
pub fn used(conn: &mut Connection, user: &str) -> io::Result<i64> {
    Ok(number(conn, &used_key(user))?.unwrap_or(0))
}

pub fn set_limit(conn: &mut Connection, user: &str, bytes: i64) -> io::Result<()> {
    conn.set(limit_key(user).as_bytes(), bytes.to_string().as_bytes())?;
    Ok(())
}

/// Give a new account the default limit, unless it already has one (a
/// limit an operator set before is kept). True when it wrote.
pub fn give_default(conn: &mut Connection, user: &str) -> io::Result<bool> {
    if limit(conn, user)?.is_some() {
        return Ok(false);
    }
    set_limit(conn, user, DEFAULT_QUOTA_BYTES)?;
    Ok(true)
}

/// Past [`NEAR_FULL_PERCENT`] of `limit`.
pub fn near_full(used: i64, limit: i64) -> bool {
    i128::from(used) * 100 > i128::from(limit) * i128::from(NEAR_FULL_PERCENT)
}

fn ratio(used: i64, limit: i64) -> f64 {
    used as f64 / limit as f64
}

/// Give the default limit to every account in `users` that has none,
/// except one already past [`NEAR_FULL_PERCENT`] of it, which is left for
/// the operator. With `dry_run` nothing is written.
pub fn fill_in(
    conn: &mut Connection,
    users: &[String],
    dry_run: bool,
) -> io::Result<QuotaLimitsResponse> {
    let mut out = QuotaLimitsResponse {
        dry_run,
        default_bytes: DEFAULT_QUOTA_BYTES,
        accounts: 0,
        written: 0,
        skipped_has_limit: 0,
        skipped_over_80: 0,
        rows: Vec::with_capacity(users.len()),
    };
    // one order whichever core listed the accounts
    let mut users = users.to_vec();
    users.sort();
    for user in &users {
        out.accounts += 1;
        let used_bytes = used(conn, user)?;
        let limit_bytes = limit(conn, user)?;
        let action = match (limit_bytes, near_full(used_bytes, DEFAULT_QUOTA_BYTES)) {
            (Some(_), _) => {
                out.skipped_has_limit += 1;
                QuotaLimitAction::HasLimit
            }
            (None, true) => {
                out.skipped_over_80 += 1;
                QuotaLimitAction::Over80
            }
            (None, false) if dry_run => {
                out.written += 1;
                QuotaLimitAction::WouldWrite
            }
            (None, false) => {
                set_limit(conn, user, DEFAULT_QUOTA_BYTES)?;
                out.written += 1;
                QuotaLimitAction::Written
            }
        };
        out.rows.push(QuotaLimitRow {
            address: user.clone(),
            used_bytes,
            limit_bytes,
            used_ratio: ratio(used_bytes, DEFAULT_QUOTA_BYTES),
            action,
        });
    }
    Ok(out)
}

/// How full the accounts are, from the counters (no maildir is read).
#[derive(Debug, Clone, PartialEq)]
pub struct Census {
    pub accounts: u64,
    /// Accounts with no limit: none of the other numbers count them.
    pub accounts_without_limit: u64,
    /// Accounts past [`NEAR_FULL_PERCENT`] of their limit.
    pub over_80: u64,
    /// The fullest account's usage / limit; `None` when no account has a
    /// limit.
    pub max_used_ratio: Option<f64>,
}

pub fn census(conn: &mut Connection, users: &[String]) -> io::Result<Census> {
    let mut out = Census {
        accounts: 0,
        accounts_without_limit: 0,
        over_80: 0,
        max_used_ratio: None,
    };
    for user in users {
        out.accounts += 1;
        let Some(lim) = limit(conn, user)? else {
            out.accounts_without_limit += 1;
            continue;
        };
        let u = used(conn, user)?;
        if near_full(u, lim) {
            out.over_80 += 1;
        }
        let r = ratio(u, lim);
        out.max_used_ratio = Some(out.max_used_ratio.map_or(r, |m| m.max(r)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> Connection {
        Connection::connect(&format!("mem://quota-limits-{name}")).unwrap()
    }

    fn users(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_new_account_gets_the_default_and_a_set_limit_is_kept() {
        let mut c = conn("default");
        assert!(give_default(&mut c, "a@x.test").unwrap());
        assert_eq!(
            limit(&mut c, "a@x.test").unwrap(),
            Some(DEFAULT_QUOTA_BYTES)
        );
        set_limit(&mut c, "b@x.test", 5).unwrap();
        assert!(!give_default(&mut c, "b@x.test").unwrap());
        assert_eq!(limit(&mut c, "b@x.test").unwrap(), Some(5));
    }

    #[test]
    fn a_dry_run_writes_nothing_and_a_write_skips_limits_and_full_accounts() {
        let mut c = conn("fill");
        set_limit(&mut c, "kept@x.test", 7).unwrap();
        let full = DEFAULT_QUOTA_BYTES / 10 * 9;
        c.set(
            used_key("full@x.test").as_bytes(),
            full.to_string().as_bytes(),
        )
        .unwrap();
        c.set(used_key("small@x.test").as_bytes(), b"1024").unwrap();
        let all = users(&["kept@x.test", "full@x.test", "small@x.test", "none@x.test"]);

        let dry = fill_in(&mut c, &all, true).unwrap();
        assert_eq!(
            (
                dry.accounts,
                dry.written,
                dry.skipped_has_limit,
                dry.skipped_over_80
            ),
            (4, 2, 1, 1)
        );
        assert_eq!(
            limit(&mut c, "small@x.test").unwrap(),
            None,
            "a dry run wrote"
        );

        let done = fill_in(&mut c, &all, false).unwrap();
        let action = |a: &str| done.rows.iter().find(|r| r.address == a).unwrap().action;
        assert_eq!(action("kept@x.test"), QuotaLimitAction::HasLimit);
        assert_eq!(action("full@x.test"), QuotaLimitAction::Over80);
        assert_eq!(action("small@x.test"), QuotaLimitAction::Written);
        assert_eq!(action("none@x.test"), QuotaLimitAction::Written);
        assert_eq!(limit(&mut c, "kept@x.test").unwrap(), Some(7));
        assert_eq!(limit(&mut c, "full@x.test").unwrap(), None);
        assert_eq!(
            limit(&mut c, "none@x.test").unwrap(),
            Some(DEFAULT_QUOTA_BYTES)
        );
    }

    #[test]
    fn the_census_counts_full_accounts_and_leaves_unlimited_ones_aside() {
        let mut c = conn("census");
        set_limit(&mut c, "a@x.test", 100).unwrap();
        c.set(used_key("a@x.test").as_bytes(), b"81").unwrap();
        set_limit(&mut c, "b@x.test", 100).unwrap();
        c.set(used_key("b@x.test").as_bytes(), b"80").unwrap();
        c.set(used_key("c@x.test").as_bytes(), b"999").unwrap();
        let got = census(&mut c, &users(&["a@x.test", "b@x.test", "c@x.test"])).unwrap();
        assert_eq!(got.accounts, 3);
        assert_eq!(got.accounts_without_limit, 1);
        assert_eq!(got.over_80, 1, "80% exactly is not past 80%");
        assert_eq!(got.max_used_ratio, Some(0.81));
    }

    #[test]
    fn no_limit_anywhere_has_no_ratio() {
        let mut c = conn("none");
        let got = census(&mut c, &users(&["a@x.test"])).unwrap();
        assert_eq!(got.max_used_ratio, None);
        assert_eq!(got.accounts_without_limit, 1);
    }
}

//! Pausing an account's outbound mail when it sends far more than it ever
//! does, or keeps failing.
//!
//! The sender counts each job when it claims it (a job is one recipient)
//! and holds the job instead of delivering it once the account is over
//! its hour's limit or paused already. Held mail is neither sent nor
//! bounced until an administrator sends or discards it, or the queue
//! lifetime runs out. Webapi and the receiver read the pause to tell
//! whoever sends that sending is paused, and whom to ask.
//!
//! Security alerts are never counted or held: they announce the pause,
//! and an alert account that paused itself would silence every alert
//! after. Only jobs written by `write_security_alert_pending` carry that
//! kind.

use std::io;

use kevy_client::Connection;

use super::keys::job_key;

/// Every paused address, for the admin listing.
pub const PAUSED_SET: &[u8] = b"outbound:paused";

pub fn paused_key(address: &str) -> String {
    format!("outbound:paused:{address}")
}

pub fn held_key(address: &str) -> String {
    format!("outbound:held:{address}")
}

fn rate_key(address: &str, hour: i64) -> String {
    format!("outbound:rate:{address}:{hour}")
}

fn fail_key(address: &str, hour: i64) -> String {
    format!("outbound:fail:{address}:{hour}")
}

/// An hour's counters outlive the hour by another, then go.
const COUNTER_TTL: std::time::Duration = std::time::Duration::from_secs(7200);

/// The ceiling one account sends under in an hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub per_hour: u64,
    pub fails_per_hour: u64,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        per_hour: 200,
        fails_per_hour: 30,
    };
}

/// Per-account ceilings from `MAILRS_OUTBOUND_PAUSE_OVERRIDES`:
/// `address:per_hour:fails_per_hour`, comma-separated. Always numbers:
/// there is no way to turn the check off for an account.
pub fn parse_overrides(raw: &str) -> Result<Vec<(String, Limits)>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|entry| {
            let mut parts = entry.split(':');
            let (Some(addr), Some(per), Some(fails), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                return Err(format!(
                    "{entry:?}: expected address:per_hour:fails_per_hour"
                ));
            };
            let number = |s: &str| {
                s.parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("{entry:?}: {s:?} is not a positive number"))
            };
            if !addr.contains('@') {
                return Err(format!("{entry:?}: {addr:?} is not an address"));
            }
            Ok((
                addr.to_ascii_lowercase(),
                Limits {
                    per_hour: number(per)?,
                    fails_per_hour: number(fails)?,
                },
            ))
        })
        .collect()
}

/// Whether `address` is paused.
pub fn is_paused(c: &mut Connection, address: &str) -> io::Result<bool> {
    Ok(c.exists(&[paused_key(address).as_bytes()])? > 0)
}

/// Count one job (one recipient) for `address` at `now`; the hour's
/// count after it.
pub fn count_send(c: &mut Connection, address: &str, now: i64) -> io::Result<u64> {
    count(c, &rate_key(address, now / 3600))
}

/// Count one permanent failure for `address` at `now`; the hour's count.
pub fn count_failure(c: &mut Connection, address: &str, now: i64) -> io::Result<u64> {
    count(c, &fail_key(address, now / 3600))
}

fn count(c: &mut Connection, key: &str) -> io::Result<u64> {
    let n = c.incr(key.as_bytes())?;
    if n == 1 {
        c.expire(key.as_bytes(), COUNTER_TTL)?;
    }
    Ok(n.max(0) as u64)
}

/// Pause `address` for `reason`. True when it was not paused before.
pub fn pause(c: &mut Connection, address: &str, reason: &str, now: i64) -> io::Result<bool> {
    let key = paused_key(address);
    if c.exists(&[key.as_bytes()])? > 0 {
        return Ok(false);
    }
    let now = now.to_string();
    c.hset(
        key.as_bytes(),
        &[
            (b"reason".as_slice(), reason.as_bytes()),
            (b"since", now.as_bytes()),
            (b"alerted_at", now.as_bytes()),
        ],
    )?;
    c.sadd(PAUSED_SET, &[address.as_bytes()])?;
    Ok(true)
}

/// Hold job `id` of `address`: not sent, not bounced, kept for an
/// administrator. Its age keeps counting from `created_at`.
pub fn hold(c: &mut Connection, address: &str, id: &str, created_at: i64) -> io::Result<()> {
    let job = job_key(id);
    c.hset(job.as_bytes(), &[(b"state".as_slice(), b"held".as_slice())])?;
    c.zadd(
        held_key(address).as_bytes(),
        &[(created_at as f64, id.as_bytes())],
    )?;
    Ok(())
}

/// One paused account as the admin listing shows it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Paused {
    pub address: String,
    pub reason: String,
    pub since: i64,
    pub held: u64,
}

pub fn paused(c: &mut Connection) -> io::Result<Vec<Paused>> {
    let mut out = Vec::new();
    for member in c.smembers(PAUSED_SET)? {
        let address = String::from_utf8_lossy(&member).to_string();
        let key = paused_key(&address);
        let reason = c.hget(key.as_bytes(), b"reason")?;
        let since = c.hget(key.as_bytes(), b"since")?;
        let (Some(reason), Some(since)) = (reason, since) else {
            // a set member whose record is gone: resumed in between
            c.srem(PAUSED_SET, &[member.as_slice()])?;
            continue;
        };
        out.push(Paused {
            held: c.zcard(held_key(&address).as_bytes())? as u64,
            reason: String::from_utf8_lossy(&reason).to_string(),
            since: String::from_utf8_lossy(&since).parse().unwrap_or(0),
            address,
        });
    }
    out.sort_by(|a, b| a.address.cmp(&b.address));
    Ok(out)
}

/// What resuming did with the held mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Resumed {
    pub sent: u64,
    pub discarded: u64,
}

/// Lift `address`'s pause: its held jobs go back to the queue when `send`,
/// or are deleted. `None` when it was not paused.
pub fn resume(
    c: &mut Connection,
    address: &str,
    send: bool,
    now: i64,
) -> io::Result<Option<Resumed>> {
    if !is_paused(c, address)? {
        return Ok(None);
    }
    let ids = held_ids(c, address, i64::MAX)?;
    let mut done = Resumed {
        sent: 0,
        discarded: 0,
    };
    for id in &ids {
        match send {
            true => {
                if let Ok(n) = id.parse::<i64>()
                    && super::requeue_pending(c, n, now)?
                {
                    done.sent += 1;
                }
            }
            false => {
                c.del(&[job_key(id).as_bytes()])?;
                done.discarded += 1;
            }
        }
    }
    c.del(&[paused_key(address).as_bytes(), held_key(address).as_bytes()])?;
    c.srem(PAUSED_SET, &[address.as_bytes()])?;
    Ok(Some(done))
}

/// Held job ids of `address` created before `before`, oldest first.
pub fn held_ids(c: &mut Connection, address: &str, before: i64) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for member in c.zrange(held_key(address).as_bytes(), 0, -1)? {
        let id = String::from_utf8_lossy(&member).to_string();
        let created = c
            .hget(job_key(&id).as_bytes(), b"created_at")?
            .and_then(|v| String::from_utf8_lossy(&v).parse::<i64>().ok());
        if created.is_none_or(|t| t < before) {
            out.push(id);
        }
    }
    Ok(out)
}

/// Take job `id` out of `address`'s held set and delete it; the blob it
/// had, for the bounce, when there was one.
pub fn drop_held(c: &mut Connection, address: &str, id: &str) -> io::Result<Option<Vec<u8>>> {
    let job = job_key(id);
    let blob = c.hget(job.as_bytes(), b"blob")?;
    c.zrem(held_key(address).as_bytes(), &[id.as_bytes()])?;
    c.del(&[job.as_bytes()])?;
    Ok(blob)
}

/// When the pause was last announced; the sender repeats it daily.
pub fn alerted_at(c: &mut Connection, address: &str) -> io::Result<Option<i64>> {
    let at = c.hget(paused_key(address).as_bytes(), b"alerted_at")?;
    Ok(at.and_then(|v| String::from_utf8_lossy(&v).parse().ok()))
}

pub fn set_alerted_at(c: &mut Connection, address: &str, now: i64) -> io::Result<()> {
    c.hset(
        paused_key(address).as_bytes(),
        &[(b"alerted_at".as_slice(), now.to_string().as_bytes())],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_are_numbers_for_named_accounts() {
        let got = parse_overrides(" news@golia.jp:2000:100 , a@b.jp:300:40").unwrap();
        assert_eq!(
            got,
            vec![
                (
                    "news@golia.jp".to_string(),
                    Limits {
                        per_hour: 2000,
                        fails_per_hour: 100
                    }
                ),
                (
                    "a@b.jp".to_string(),
                    Limits {
                        per_hour: 300,
                        fails_per_hour: 40
                    }
                ),
            ]
        );
        assert_eq!(parse_overrides("").unwrap(), vec![]);
        for bad in [
            "news@golia.jp:off:1",
            "news@golia.jp:0:1",
            "news@golia.jp:5",
            "nobody:5:5",
        ] {
            assert!(parse_overrides(bad).is_err(), "{bad} was taken");
        }
    }
}

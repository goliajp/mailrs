//! The chain's own thread, started by whichever core a deployment runs:
//! seal every [`TICK`], and once a day verify, compare with the replica,
//! write the new rows to it, move the anchor up, and record the outcome
//! for `/api/health`. A broken chain or a failed replica raises an alert.
//!
//! The daily check cannot be incremental — it exists to recompute what is
//! already sealed — so it does its whole work every day: every row the log
//! holds (at most `audit::CAP` × 1.1, about 55 000) is read and hashed
//! once, a few seconds of one core. Sealing in between converges: with
//! nothing appended, a tick reads a counter and a head and writes nothing.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kevy_client::Connection;

use super::{Sealer, compare_replica, prune, verify, write_replica};
use crate::keys::admin::AUDIT_CHAIN_STATUS;

/// How often new rows are sealed.
pub const TICK: Duration = Duration::from_secs(10);

/// The outcome of the last daily check, as `/api/health` reports it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Status {
    /// Unix seconds.
    pub verified_at: i64,
    /// The chain verified and agreed with the replica.
    pub ok: bool,
    /// Sealed rows checked.
    pub rows: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_bad_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// `off` (no replica directory configured), `ok` or `failed`.
    pub replica: String,
    /// Why the replica could not be written, when it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replica_error: Option<String>,
}

/// The last daily outcome; `None` before the first.
pub fn read_status(c: &mut Connection) -> io::Result<Option<Status>> {
    Ok(c.get(AUDIT_CHAIN_STATUS)?
        .and_then(|v| serde_json::from_slice(&v).ok()))
}

/// One daily check, recorded. `cap` is the log's retention (`audit::CAP`).
pub fn daily(c: &mut Connection, cap: i64, replica: Option<&Path>, now: i64) -> io::Result<Status> {
    let report = verify(c, cap)?;
    let mut status = Status {
        verified_at: now,
        ok: report.ok,
        rows: report.rows,
        first_bad_id: report.first_bad_id,
        reason: report.reason,
        replica: "off".into(),
        replica_error: None,
    };
    if let Some(dir) = replica {
        match compare_replica(dir, c) {
            Ok(Ok(_)) => status.replica = "ok".into(),
            Ok(Err((id, why))) => {
                status.ok = false;
                status.replica = "failed".into();
                if status.reason.is_none() {
                    status.first_bad_id = id;
                    status.reason = Some(why);
                }
            }
            Err(e) => {
                status.replica = "failed".into();
                status.replica_error = Some(format!("reading {}: {e}", dir.display()));
            }
        }
        if status.ok
            && let Err(e) = write_replica(dir, c)
        {
            status.replica = "failed".into();
            status.replica_error = Some(format!("writing {}: {e}", dir.display()));
        }
    }
    if status.ok {
        prune(c, cap)?;
    }
    let json = serde_json::to_vec(&status).map_err(io::Error::other)?;
    c.set(AUDIT_CHAIN_STATUS, &json)?;
    Ok(status)
}

/// `HH:MM` in UTC.
fn parse_at(raw: &str) -> Option<(u32, u32)> {
    let (h, m) = raw.trim().split_once(':')?;
    let (h, m) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// The first time after `now` (unix seconds) that the clock reads `at`.
fn next_at(now: i64, at: (u32, u32)) -> i64 {
    let day = now - now.rem_euclid(86_400);
    let today = day + i64::from(at.0) * 3600 + i64::from(at.1) * 60;
    match today > now {
        true => today,
        false => today + 86_400,
    }
}

fn raise(c: &mut Connection, status: &Status, now: i64) {
    use mailrs_auth_guard::alerts::{Alert, Kind};
    let detail = match (&status.reason, &status.replica_error) {
        (Some(why), _) => match status.first_bad_id {
            Some(id) => format!("row {id}: {why}"),
            None => why.clone(),
        },
        (None, Some(why)) => why.clone(),
        (None, None) => return,
    };
    let alert = Alert {
        kind: Kind::AuditChainBroken,
        subject: std::env::var("MAILRS_HOSTNAME").unwrap_or_else(|_| "mailrs".into()),
        at: now.max(0) as u64,
        detail,
        facts: Vec::new(),
    };
    if let Err(e) = mailrs_auth_guard::alerts::raise(c, &alert) {
        tracing::error!(error = %e, "audit chain alert not raised");
    }
}

/// Start the thread. `connect` opens the network kevy; `cap` is the log's
/// retention. Reads `MAILRS_AUDIT_VERIFY_AT` (`HH:MM` UTC, 03:30 by
/// default) and `MAILRS_AUDIT_REPLICA_DIR` (no replica when unset).
pub fn spawn<F>(connect: F, cap: i64)
where
    F: Fn() -> Option<Connection> + Send + 'static,
{
    let raw_at = std::env::var("MAILRS_AUDIT_VERIFY_AT").unwrap_or_else(|_| "03:30".into());
    let at = parse_at(&raw_at);
    if at.is_none() {
        tracing::error!(
            value = %raw_at,
            "MAILRS_AUDIT_VERIFY_AT: expected HH:MM in UTC; the audit chain is sealed but not verified"
        );
    }
    let replica: Option<PathBuf> = std::env::var("MAILRS_AUDIT_REPLICA_DIR")
        .ok()
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let spawned = std::thread::Builder::new()
        .name("audit-chain".into())
        .spawn(move || run(connect, cap, at, replica));
    if let Err(e) = spawned {
        tracing::error!(error = %e, "audit chain thread not started");
    }
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

fn run<F>(connect: F, cap: i64, at: Option<(u32, u32)>, replica: Option<PathBuf>)
where
    F: Fn() -> Option<Connection>,
{
    let mut sealer = Sealer::default();
    let mut conn: Option<Connection> = None;
    let mut due = at.map(|a| next_at(now_secs(), a));
    loop {
        std::thread::sleep(TICK);
        if conn.is_none() {
            conn = connect();
        }
        let Some(c) = conn.as_mut() else {
            continue;
        };
        if let Err(e) = sealer.tick(c, Instant::now()) {
            tracing::warn!(error = %e, "audit chain: sealing failed");
            conn = None;
            continue;
        }
        let now = now_secs();
        let (Some(when), Some(a)) = (due, at) else {
            continue;
        };
        if now < when {
            continue;
        }
        due = Some(next_at(now, a));
        match daily(c, cap, replica.as_deref(), now) {
            Ok(s) if s.ok && s.replica_error.is_none() => {
                tracing::info!(rows = s.rows, replica = %s.replica, "audit chain verified");
            }
            Ok(s) => {
                tracing::error!(?s, "audit chain check failed");
                raise(c, &s, now);
            }
            Err(e) => {
                tracing::error!(error = %e, "audit chain check could not run");
                conn = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daily_time_is_the_next_one_to_come() {
        let midnight = 1_793_404_800; // a UTC midnight
        assert_eq!(parse_at("03:30"), Some((3, 30)));
        assert_eq!(parse_at("24:00"), None);
        assert_eq!(parse_at("3"), None);
        assert_eq!(next_at(midnight, (3, 30)), midnight + 12_600);
        assert_eq!(
            next_at(midnight + 12_600, (3, 30)),
            midnight + 86_400 + 12_600
        );
        assert_eq!(
            next_at(midnight + 20_000, (3, 30)),
            midnight + 86_400 + 12_600
        );
    }
}

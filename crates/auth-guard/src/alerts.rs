//! Security alerts the login guard raises, for whoever operates the
//! server. Raising one only queues it (`auth:alerts:pending`); one process
//! takes the queue and mails it. The same kind of alert about the same
//! account or source is queued at most once per [`REPEAT_SECS`], so an
//! attack yields a handful of lines, not a mailbox full.

use std::io;

use serde::{Deserialize, Serialize};

use crate::events::EventOps;
use crate::login::Ops;

/// The queue alerts wait in until they are mailed.
pub const PENDING: &str = "auth:alerts:pending";
/// The same alert about the same subject is raised once in this long.
pub const REPEAT_SECS: u64 = 3600;
/// Alerts still unsent after this long are dropped with their queue.
const PENDING_TTL: u64 = 24 * 3600;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Failures from many sources against one account locked it for
    /// unfamiliar sources (someone rotating addresses to guess it).
    AccountLocked,
    /// One source failed against many accounts and was locked.
    SourceLocked,
    /// Failures across the server crossed the global threshold: unfamiliar
    /// sources are refused until the window has room again.
    GlobalLocked,
    /// One account signed in successfully from several networks within a
    /// few minutes.
    ManyNetworks,
    /// A signed-in session failed to confirm its password too many times
    /// and was ended: whoever held it did not know the password.
    SessionEndedAfterFailedConfirmations,
    /// A code to set a new password on an account whose password was
    /// cleared was handed out. Sent every time, not once an hour: each code
    /// is a way into an account.
    ResetCodeIssued,
}

impl Kind {
    /// Whether a repeat about the same subject within [`REPEAT_SECS`] is
    /// dropped.
    fn once_an_hour(self) -> bool {
        !matches!(self, Kind::ResetCodeIssued)
    }
}

/// One alert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    /// What happened.
    pub kind: Kind,
    /// The account or source it is about (`*` for the global count).
    pub subject: String,
    /// Unix seconds.
    pub at: u64,
    /// The facts behind it, for the reader.
    pub detail: String,
}

/// Queue `alert` unless the same kind about the same subject was queued
/// within [`REPEAT_SECS`]. Whether it was queued.
pub fn raise<O: Ops + EventOps>(ops: &mut O, alert: &Alert) -> io::Result<bool> {
    let kind = serde_json::to_value(alert.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let once = format!("auth:alerts:once:{kind}:{}", alert.subject);
    if alert.kind.once_an_hour() && Ops::incr(ops, &once, REPEAT_SECS)? > 1 {
        return Ok(false);
    }
    let row = serde_json::to_vec(alert).map_err(io::Error::other)?;
    ops.push(PENDING, &row, PENDING_TTL)?;
    tracing::warn!(event = "security_alert", kind, subject = %alert.subject, detail = %alert.detail);
    Ok(true)
}

/// Up to `max` queued alerts, oldest first, removed from the queue.
pub fn take<O: EventOps>(ops: &mut O, max: usize) -> io::Result<Vec<Alert>> {
    let mut out = Vec::new();
    while out.len() < max {
        let Some(row) = ops.pop_oldest(PENDING)? else {
            break;
        };
        if let Ok(alert) = serde_json::from_slice(&row) {
            out.push(alert);
        }
    }
    Ok(out)
}

/// The mail an operator gets for `alerts`, as (subject, body).
pub fn digest(alerts: &[Alert], host: &str) -> (String, String) {
    let subject = match alerts {
        [one] => format!("[{host}] security: {}", headline(one)),
        many => format!("[{host}] security: {} alerts", many.len()),
    };
    let mut body = format!(
        "The login guard on {host} raised the following. Each kind of alert about one account or source is sent at most once an hour, except reset codes, every one of which is.\n\n"
    );
    for a in alerts {
        body.push_str(&format!(
            "- {} UTC  {}\n  {}\n",
            utc(a.at),
            headline(a),
            a.detail
        ));
    }
    body.push_str("\nThe sign-in attempts behind them: Admin → Oversight → Sign-ins, or GET /api/admin/auth-events.\n");
    (subject, body)
}

fn headline(a: &Alert) -> String {
    match a.kind {
        Kind::AccountLocked => format!("account {} locked for unfamiliar sources", a.subject),
        Kind::SourceLocked => format!("source {} locked", a.subject),
        Kind::GlobalLocked => "unfamiliar sources refused server-wide".to_string(),
        Kind::ManyNetworks => format!("{} signed in from several networks", a.subject),
        Kind::SessionEndedAfterFailedConfirmations => {
            format!(
                "a session of {} ended after failed password confirmations",
                a.subject
            )
        }
        Kind::ResetCodeIssued => format!("a password reset code was issued for {}", a.subject),
    }
}

fn utc(unix: u64) -> String {
    let d = crate::events::day(unix);
    let s = unix % 86_400;
    format!(
        "{}-{:02}-{:02} {:02}:{:02}",
        d / 10_000,
        d / 100 % 100,
        d % 100,
        s / 3600,
        s / 60 % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login::MemoryOps;

    fn alert(kind: Kind, subject: &str) -> Alert {
        Alert {
            kind,
            subject: subject.into(),
            at: 1_792_892_646,
            detail: "10 failures in an hour from 10 sources".into(),
        }
    }

    #[test]
    fn the_same_alert_is_queued_once_an_hour() {
        let mut o = MemoryOps::default();
        o.at(1_792_892_646);
        assert!(raise(&mut o, &alert(Kind::AccountLocked, "a@x.test")).unwrap());
        assert!(!raise(&mut o, &alert(Kind::AccountLocked, "a@x.test")).unwrap());
        assert!(raise(&mut o, &alert(Kind::AccountLocked, "b@x.test")).unwrap());
        assert!(raise(&mut o, &alert(Kind::ManyNetworks, "a@x.test")).unwrap());
        o.at(1_792_892_646 + REPEAT_SECS);
        assert!(raise(&mut o, &alert(Kind::AccountLocked, "a@x.test")).unwrap());
        let taken = take(&mut o, 100).unwrap();
        let subjects: Vec<&str> = taken.iter().map(|a| a.subject.as_str()).collect();
        assert_eq!(
            subjects,
            vec!["a@x.test", "b@x.test", "a@x.test", "a@x.test"],
            "oldest first"
        );
        assert!(take(&mut o, 100).unwrap().is_empty(), "taken means gone");
    }

    #[test]
    fn every_reset_code_is_reported() {
        let mut o = MemoryOps::default();
        o.at(1_792_892_646);
        assert!(raise(&mut o, &alert(Kind::ResetCodeIssued, "a@x.test")).unwrap());
        assert!(raise(&mut o, &alert(Kind::ResetCodeIssued, "a@x.test")).unwrap());
        assert_eq!(take(&mut o, 100).unwrap().len(), 2);
    }

    #[test]
    fn a_digest_names_each_alert() {
        let (subject, body) = digest(&[alert(Kind::AccountLocked, "a@x.test")], "mail.golia.ai");
        assert_eq!(
            subject,
            "[mail.golia.ai] security: account a@x.test locked for unfamiliar sources"
        );
        assert!(body.contains("2026-10-25 01:44 UTC"), "{body}");
        let (subject, _) = digest(
            &[
                alert(Kind::SourceLocked, "192.0.2.1"),
                alert(Kind::GlobalLocked, "*"),
            ],
            "mail.golia.ai",
        );
        assert_eq!(subject, "[mail.golia.ai] security: 2 alerts");
    }
}

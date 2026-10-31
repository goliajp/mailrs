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
    /// The daily backup failed, or none has succeeded for too long.
    BackupFailed,
    /// An account's outbound mail was paused: it sent far more in an hour
    /// than its limit, or kept failing. Repeated daily while it lasts.
    OutboundPaused,
    /// A feedback-loop reporter sent more matched complaints in a day than
    /// are acted on; the rest are logged and suppress nobody.
    ComplaintLimit,
}

impl Kind {
    /// Whether a repeat about the same subject within [`REPEAT_SECS`] is
    /// dropped.
    fn once_an_hour(self) -> bool {
        !matches!(self, Kind::ResetCodeIssued)
    }
}

/// One fact behind an alert, typed so the mail can say it in the reader's
/// language and time zone. Times are unix seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "fact", rename_all = "kebab-case")]
pub enum Fact {
    /// Failed sign-ins in a window.
    Failures {
        /// How many failed.
        count: u64,
        /// Within this many minutes.
        minutes: u64,
    },
    /// The last failed attempt.
    LastAttempt {
        /// The address it came from.
        source: String,
        /// The account it tried.
        account: String,
    },
    /// A lock and how long it holds.
    LockedFor {
        /// Minutes.
        minutes: u64,
    },
    /// Successful sign-ins from several networks.
    Networks {
        /// The networks.
        list: Vec<String>,
        /// Within this many minutes.
        minutes: u64,
    },
    /// Who issued it.
    IssuedBy {
        /// Their address.
        actor: String,
    },
    /// Valid until then.
    ValidUntil {
        /// Unix seconds.
        at: u64,
    },
    /// A session that failed its password confirmations.
    Session {
        /// Its first characters.
        prefix: String,
        /// Where it was used from.
        source: String,
        /// Confirmations it failed.
        failures: u64,
    },
    /// Recipients sent to in an hour, over the account's limit.
    SendLimit {
        /// How many.
        count: u64,
        /// The limit.
        limit: u64,
    },
    /// Permanent delivery failures in an hour, over the account's limit.
    FailureLimit {
        /// How many.
        count: u64,
        /// The limit.
        limit: u64,
    },
    /// Paused since then.
    PausedSince {
        /// Unix seconds.
        at: u64,
    },
    /// Held messages thrown away when the queue gave up on them.
    Discarded {
        /// How many.
        count: u64,
    },
    /// No backup has succeeded for this long.
    NoBackupFor {
        /// Hours.
        hours: u64,
    },
    /// More complaints from a feedback loop today than are acted on.
    Complaints {
        /// The reporter.
        reporter: String,
        /// Complaints a day that are acted on.
        limit: u64,
    },
    /// The address the latest complaint was about.
    LatestComplaint {
        /// The address.
        address: String,
    },
    /// Words from the system that raised it, for the operator.
    Note {
        /// The words.
        text: String,
    },
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
    /// The same facts in one English line, for logs and for alerts queued
    /// before `facts` existed.
    pub detail: String,
    /// The facts behind it.
    #[serde(default)]
    pub facts: Vec<Fact>,
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
            facts: vec![Fact::Failures {
                count: 10,
                minutes: 60,
            }],
        }
    }

    /// An alert queued by the previous build has no `facts`, and still
    /// reads: during a deploy the queue holds both.
    #[test]
    fn an_alert_without_facts_still_reads() {
        let old = r#"{"kind":"account-locked","subject":"a@x.test","at":1,"detail":"d"}"#;
        let a: Alert = serde_json::from_str(old).unwrap();
        assert_eq!((a.kind, a.facts.len()), (Kind::AccountLocked, 0));
        let new = serde_json::to_string(&alert(Kind::AccountLocked, "a@x.test")).unwrap();
        assert!(
            new.contains(r#""facts":[{"fact":"failures","count":10,"minutes":60}]"#),
            "{new}"
        );
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
}

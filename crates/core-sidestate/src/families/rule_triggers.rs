//! Each delivery a rule under review fired on, kept so the review can list
//! them: who it was for, which domain sent it, what the rule saw, when.
//!
//! ```text
//! mailrs:fraud:triggers:{rule}:{yyyymmdd}   list of JSON lines, one per
//!                                           delivery; expires after
//!                                           KEEP_DAYS days
//! ```
//!
//! [`super::fraud_hits`] counts hits; this keeps them one by one, because a
//! review asks whether each one was right. The receiver writes it for the
//! rules in [`REVIEWED`] only, at delivery time; a rejudge of old mail does
//! not. Kept outside the container's log, which a deploy starts over.

use std::io;
use std::time::Duration;

use kevy_client::Connection;
use serde::{Deserialize, Serialize};

/// The rules whose every trigger is kept while they are new.
pub const REVIEWED: &[&str] = &["burner-domain-outreach"];

/// Days a day's list is kept: a 30-day review plus time to hold it late.
pub const KEEP_DAYS: i64 = 60;

/// One delivery a reviewed rule fired on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    /// Unix seconds it was received.
    pub at: i64,
    /// The envelope recipients it was delivered to.
    pub recipients: Vec<String>,
    /// The `From` domain.
    pub from_domain: String,
    /// What the rule went by beyond the sender: for
    /// `burner-domain-outreach`, the host the sender's web root landed on.
    pub detail: String,
    pub message_id: String,
}

/// The list of `rule`'s triggers on UTC day `day` (`yyyymmdd`).
pub fn key(rule: &str, day: &str) -> String {
    format!("mailrs:fraud:triggers:{rule}:{day}")
}

/// Keep one trigger of `rule`, on the day it was received.
pub fn record(conn: &mut Connection, rule: &str, trigger: &Trigger) -> io::Result<()> {
    let k = key(rule, &super::fraud_hits::day(trigger.at, 0));
    let line = serde_json::to_vec(trigger).map_err(io::Error::other)?;
    conn.rpush(k.as_bytes(), &[line.as_slice()])?;
    conn.expire(
        k.as_bytes(),
        Duration::from_secs((KEEP_DAYS * 86_400) as u64),
    )?;
    Ok(())
}

/// `rule`'s triggers over the `days` UTC days up to the one holding `now`,
/// oldest day first. A line that does not parse is skipped.
pub fn list(conn: &mut Connection, rule: &str, now: i64, days: i64) -> io::Result<Vec<Trigger>> {
    let mut found = Vec::new();
    for ago in (0..days).rev() {
        let k = key(rule, &super::fraud_hits::day(now, ago));
        for line in conn.lrange(k.as_bytes(), 0, -1)? {
            if let Ok(t) = serde_json::from_slice::<Trigger>(&line) {
                found.push(t);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> Connection {
        Connection::connect(&format!("mem://rule-triggers-{name}")).unwrap()
    }

    const NOW: i64 = 1_793_750_400; // 2026-11-04 00:00:00 UTC

    fn trigger(at: i64, from: &str) -> Trigger {
        Trigger {
            at,
            recipients: vec!["a@golia.jp".into()],
            from_domain: from.into(),
            detail: "dealclosers.co".into(),
            message_id: format!("<{from}@x>"),
        }
    }

    #[test]
    fn each_trigger_is_kept_and_listed_oldest_day_first() {
        let mut c = conn("list");
        let rule = REVIEWED[0];
        record(&mut c, rule, &trigger(NOW - 3 * 86_400, "old.com")).unwrap();
        record(&mut c, rule, &trigger(NOW, "today.com")).unwrap();
        record(&mut c, rule, &trigger(NOW + 60, "today2.com")).unwrap();
        let got = list(&mut c, rule, NOW + 120, 30).unwrap();
        let froms: Vec<&str> = got.iter().map(|t| t.from_domain.as_str()).collect();
        assert_eq!(froms, ["old.com", "today.com", "today2.com"]);
        assert_eq!(got[1], trigger(NOW, "today.com"));
    }

    #[test]
    fn the_window_leaves_out_older_days_and_other_rules() {
        let mut c = conn("window");
        record(&mut c, REVIEWED[0], &trigger(NOW - 10 * 86_400, "old.com")).unwrap();
        record(&mut c, "another-rule", &trigger(NOW, "other.com")).unwrap();
        assert!(list(&mut c, REVIEWED[0], NOW, 7).unwrap().is_empty());
        assert_eq!(list(&mut c, REVIEWED[0], NOW, 11).unwrap().len(), 1);
    }
}

//! How often each enforced fraud rule fired, per UTC day, in the shared
//! store: the receiver adds one per hit at delivery time, `/api/health`
//! reads the counts so a watch can fire on any hit of a rule under
//! review. Shadow rules are not counted here; their hits are only logged.
//!
//! A day's key lives [`KEEP_DAYS`] days, so the 30-day window always has
//! every day it sums, and nothing has to sweep old keys.

use std::collections::BTreeMap;
use std::io;
use std::time::Duration;

use kevy_client::Connection;

/// Days a day's counter is kept: the 30-day window plus margin.
pub const KEEP_DAYS: i64 = 35;

/// Days summed for the long window.
pub const WINDOW_DAYS: i64 = 30;

/// The counter of `rule`'s hits on UTC day `day` (`yyyymmdd`).
pub fn key(rule: &str, day: &str) -> String {
    format!("mailrs:fraud:hits:{rule}:{day}")
}

/// The UTC day `days_ago` days before `now` (unix seconds), as `yyyymmdd`.
pub fn day(now: i64, days_ago: i64) -> String {
    let at = now - days_ago * 86_400;
    chrono::DateTime::from_timestamp(at, 0)
        .unwrap_or_default()
        .format("%Y%m%d")
        .to_string()
}

/// Count one hit for each of `rules` on today's counter.
pub fn record(conn: &mut Connection, rules: &[String], now: i64) -> io::Result<()> {
    let today = day(now, 0);
    let keep = Duration::from_secs((KEEP_DAYS * 86_400) as u64);
    for rule in rules {
        let k = key(rule, &today);
        conn.incr(k.as_bytes())?;
        conn.expire(k.as_bytes(), keep)?;
    }
    Ok(())
}

/// Hits per rule: today (UTC) and over the last [`WINDOW_DAYS`] days,
/// today included. Every rule asked for is present, at 0 when it never
/// fired, so a watch on one rule always has a number to compare.
#[derive(Debug, Default, PartialEq)]
pub struct Counts {
    pub today: BTreeMap<String, i64>,
    pub last_30d: BTreeMap<String, i64>,
}

pub fn counts(conn: &mut Connection, rules: &[String], now: i64) -> io::Result<Counts> {
    let days: Vec<String> = (0..WINDOW_DAYS).map(|d| day(now, d)).collect();
    let mut out = Counts::default();
    for rule in rules {
        let keys: Vec<String> = days.iter().map(|d| key(rule, d)).collect();
        let refs: Vec<&[u8]> = keys.iter().map(|k| k.as_bytes()).collect();
        let values = conn.mget(&refs)?;
        let numbers: Vec<i64> = values
            .into_iter()
            .map(|v| {
                v.and_then(|b| String::from_utf8(b).ok())
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .unwrap_or(0)
            })
            .collect();
        out.today
            .insert(rule.clone(), numbers.first().copied().unwrap_or(0));
        out.last_30d.insert(rule.clone(), numbers.iter().sum());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> Connection {
        Connection::connect(&format!("mem://fraud-hits-{name}")).unwrap()
    }

    const NOW: i64 = 1_793_750_400; // 2026-11-04 00:00:00 UTC

    #[test]
    fn a_hit_is_counted_today_and_in_the_window() {
        let mut c = conn("count");
        let rules = vec!["svg-runs-script".to_string()];
        record(&mut c, &rules, NOW).unwrap();
        record(&mut c, &rules, NOW + 60).unwrap();
        let got = counts(&mut c, &rules, NOW + 120).unwrap();
        assert_eq!(got.today["svg-runs-script"], 2);
        assert_eq!(got.last_30d["svg-runs-script"], 2);
    }

    #[test]
    fn a_rule_that_never_fired_reads_zero() {
        let mut c = conn("zero");
        let rules = vec!["html-builds-a-download".to_string()];
        let got = counts(&mut c, &rules, NOW).unwrap();
        assert_eq!(got.today["html-builds-a-download"], 0);
        assert_eq!(got.last_30d["html-builds-a-download"], 0);
    }

    #[test]
    fn yesterday_counts_in_the_window_but_not_today() {
        let mut c = conn("days");
        let rules = vec!["bec-pay-to-a-stranger".to_string()];
        record(&mut c, &rules, NOW - 3_600).unwrap();
        let got = counts(&mut c, &rules, NOW + 3_600).unwrap();
        assert_eq!(got.today["bec-pay-to-a-stranger"], 0);
        assert_eq!(got.last_30d["bec-pay-to-a-stranger"], 1);
    }

    #[test]
    fn a_day_thirty_days_back_is_outside_the_window() {
        let mut c = conn("window");
        let rules = vec!["new-domain-login-link".to_string()];
        record(&mut c, &rules, NOW - WINDOW_DAYS * 86_400).unwrap();
        let got = counts(&mut c, &rules, NOW).unwrap();
        assert_eq!(got.last_30d["new-domain-login-link"], 0);
    }

    #[test]
    fn the_key_names_the_rule_and_the_utc_day() {
        assert_eq!(
            key("svg-runs-script", &day(NOW, 0)),
            "mailrs:fraud:hits:svg-runs-script:20261104"
        );
        assert_eq!(day(NOW, 1), "20261103");
    }
}

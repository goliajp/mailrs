//! A recurrence rule read for showing in words, and its `UNTIL`.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use super::clock::utc_to_local;
use super::{Moment, Series, Zone};

/// A rule, read for showing in words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// `DAILY` / `WEEKLY` / `MONTHLY` / `YEARLY` / ….
    pub freq: String,
    /// Every how many `freq`s.
    pub interval: u32,
    /// `BYDAY` as written: `TH`, `2TU`, `-1FR`.
    pub by_day: Vec<String>,
    /// `BYMONTHDAY`.
    pub by_month_day: Vec<i32>,
    /// `BYMONTH`.
    pub by_month: Vec<u32>,
    /// `COUNT`.
    pub count: Option<u32>,
    /// The last day it can occur on, in the organiser's zone.
    pub until: Option<NaiveDate>,
    /// Nothing beyond the parts above, so it can be said in a few words.
    pub plain: bool,
}

impl Series {
    /// The rule, read for showing in words; `None` without an `RRULE`.
    pub fn rule(&self) -> Option<Rule> {
        let raw = self.rrule.as_deref()?;
        let zone = self.zone()?;
        let mut rule = Rule {
            freq: String::new(),
            interval: 1,
            by_day: Vec::new(),
            by_month_day: Vec::new(),
            by_month: Vec::new(),
            count: None,
            until: None,
            plain: true,
        };
        for part in raw.split(';') {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            match key.trim().to_ascii_uppercase().as_str() {
                "FREQ" => rule.freq = value.trim().to_ascii_uppercase(),
                "INTERVAL" => rule.interval = value.trim().parse().unwrap_or(1),
                "BYDAY" => rule.by_day = list(value, |s| Some(s.to_ascii_uppercase())),
                "BYMONTHDAY" => rule.by_month_day = list(value, |s| s.parse().ok()),
                "BYMONTH" => rule.by_month = list(value, |s| s.parse().ok()),
                "COUNT" => rule.count = value.trim().parse().ok(),
                "UNTIL" => rule.until = Until::parse(value).map(|u| u.last_day(&zone)),
                "WKST" => {}
                _ => rule.plain = false,
            }
        }
        Some(rule)
    }
}

/// `UNTIL`, in whichever of its three forms the producer wrote.
pub(super) enum Until {
    Instant(DateTime<Utc>),
    Local(NaiveDateTime),
    Date(NaiveDate),
}

impl Until {
    fn parse(value: &str) -> Option<Until> {
        let v = value.trim();
        if let Some(utc) = v.strip_suffix('Z') {
            let t = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
            return Some(Until::Instant(t.and_utc()));
        }
        if let Ok(t) = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S") {
            return Some(Until::Local(t));
        }
        NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(Until::Date)
    }

    /// Whether an occurrence at `local` (resolved to `m`) is past it.
    pub(super) fn is_before(&self, local: NaiveDateTime, m: &Moment) -> bool {
        match self {
            Until::Instant(t) => match m {
                Moment::Instant(i) => i > t,
                Moment::Date(d) => *d > t.date_naive(),
            },
            Until::Local(t) => local > *t,
            Until::Date(d) => local.date() > *d,
        }
    }

    fn last_day(&self, zone: &Zone) -> NaiveDate {
        match (self, zone) {
            (Until::Instant(t), Zone::Tz(rt)) => utc_to_local(rt, t.naive_utc()).date(),
            (Until::Instant(t), _) => t.date_naive(),
            (Until::Local(t), _) => t.date(),
            (Until::Date(d), _) => *d,
        }
    }
}

/// The rule without its `UNTIL`, and the `UNTIL`. The rule is expanded
/// in local time written as if it were UTC, so a real UTC `UNTIL` would
/// be compared against the wrong clock; it is applied here instead.
pub(super) fn split_until(raw: &str) -> (String, Option<Until>) {
    let mut until = None;
    let rest: Vec<&str> = raw
        .split(';')
        .filter(|part| match part.split_once('=') {
            Some((k, v)) if k.trim().eq_ignore_ascii_case("UNTIL") => {
                until = Until::parse(v);
                false
            }
            _ => true,
        })
        .collect();
    (rest.join(";"), until)
}

fn list<T>(value: &str, f: impl Fn(&str) -> Option<T>) -> Vec<T> {
    value.split(',').filter_map(|s| f(s.trim())).collect()
}

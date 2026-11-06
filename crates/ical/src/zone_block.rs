//! A `VTIMEZONE` written out for an IANA zone, for text that names the
//! zone but carries no definition of it (RFC 5545 §3.6.5 requires one
//! for every `TZID` a calendar uses).
//!
//! The zone database has transitions, not rules, so the rules are read
//! back out of the transitions: each run of yearly changes that happen in
//! the same month, on the same weekday of it, at the same wall-clock time
//! becomes one observance with a yearly `RRULE`. A run that is still going
//! at the end of the years scanned is left open, so a series without an
//! end keeps the zone's current rule.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::{OffsetComponents, OffsetName, Tz};

use crate::serialize::{format_naive, format_utc};
use crate::{RawComponent, RawProperty, VTimezone};

/// How many years from the first one are scanned for transitions.
const SCAN_YEARS: i32 = 12;

const WEEKDAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

#[derive(Debug, Clone)]
struct Change {
    /// The instant the new offset takes effect.
    at: DateTime<Utc>,
    from: i32,
    to: i32,
    dst: bool,
    name: Option<String>,
}

impl Change {
    /// When it happens on the clock before it, as an observance's
    /// `DTSTART` is written.
    fn onset(&self) -> NaiveDateTime {
        self.at.naive_utc() + Duration::seconds(i64::from(self.from))
    }
}

/// Which day of its month a change falls on, as `BYDAY` can say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nth {
    Nth(u32),
    Last,
}

fn nths(day: NaiveDate) -> Vec<Nth> {
    let mut out = Vec::new();
    let n = (day.day() - 1) / 7 + 1;
    if n <= 4 {
        out.push(Nth::Nth(n));
    }
    if (day + Duration::days(7)).month() != day.month() {
        out.push(Nth::Last);
    }
    out
}

/// One observance: a change, and the later yearly changes its rule gives.
struct Run {
    changes: Vec<Change>,
    nths: Vec<Nth>,
}

impl Run {
    fn takes(&self, c: &Change) -> bool {
        let Some(last) = self.changes.last() else {
            return false;
        };
        let (a, b) = (last.onset(), c.onset());
        let shared: Vec<Nth> = nths(b.date())
            .into_iter()
            .filter(|n| self.nths.contains(n))
            .collect();
        (last.from, last.to, last.dst) == (c.from, c.to, c.dst)
            && b.year() == a.year() + 1
            && b.month() == a.month()
            && b.weekday() == a.weekday()
            && b.time() == a.time()
            && !shared.is_empty()
    }
}

/// The offset in force at `t`, whether it is daylight time, and its
/// abbreviation.
fn offset_at(tz: Tz, t: DateTime<Utc>) -> (i32, bool, Option<String>) {
    let o = tz.offset_from_utc_datetime(&t.naive_utc());
    (
        o.fix().local_minus_utc(),
        !o.dst_offset().is_zero(),
        o.abbreviation().map(str::to_string),
    )
}

/// Every change of offset in `[from, to)`. Days are stepped and each
/// change found to the second; no zone changes its offset twice in a day.
fn changes(tz: Tz, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Change> {
    let mut out = Vec::new();
    let mut t = from;
    let mut before = offset_at(tz, t).0;
    while t < to {
        let next = t + Duration::days(1);
        let after = offset_at(tz, next).0;
        if after != before {
            let (mut lo, mut hi) = (t, next);
            while hi - lo > Duration::seconds(1) {
                let mid = lo + (hi - lo) / 2;
                match offset_at(tz, mid).0 == before {
                    true => lo = mid,
                    false => hi = mid,
                }
            }
            let (to_offset, dst, name) = offset_at(tz, hi);
            out.push(Change {
                at: hi,
                from: before,
                to: to_offset,
                dst,
                name,
            });
        }
        before = after;
        t = next;
    }
    out
}

fn utc_offset(secs: i32) -> String {
    let sign = match secs < 0 {
        true => '-',
        false => '+',
    };
    let a = secs.unsigned_abs();
    let (h, m, s) = (a / 3600, a % 3600 / 60, a % 60);
    match s {
        0 => format!("{sign}{h:02}{m:02}"),
        _ => format!("{sign}{h:02}{m:02}{s:02}"),
    }
}

fn prop(name: &str, value: String) -> RawProperty {
    RawProperty {
        name: name.into(),
        params: Vec::new(),
        value,
    }
}

fn observance(first: &Change, rrule: Option<String>) -> RawComponent {
    let name = match first.dst {
        true => "DAYLIGHT",
        false => "STANDARD",
    };
    let mut properties = vec![
        prop("DTSTART", format_naive(&first.onset())),
        prop("TZOFFSETFROM", utc_offset(first.from)),
        prop("TZOFFSETTO", utc_offset(first.to)),
    ];
    if let Some(n) = &first.name {
        properties.push(prop("TZNAME", n.clone()));
    }
    if let Some(r) = rrule {
        properties.push(prop("RRULE", r));
    }
    RawComponent {
        name: name.into(),
        properties,
        children: Vec::new(),
    }
}

/// A `VTIMEZONE` named `tz`'s IANA name, correct from the start of
/// `from_year` on.
pub(crate) fn vtimezone_for(tz: Tz, from_year: i32) -> VTimezone {
    let start = Utc
        .with_ymd_and_hms(from_year, 1, 1, 0, 0, 0)
        .single()
        .unwrap_or_default();
    let end = Utc
        .with_ymd_and_hms(from_year + SCAN_YEARS, 1, 1, 0, 0, 0)
        .single()
        .unwrap_or_default();
    // the offset at the start of the scan, so every time in it is covered
    let (offset, dst, name) = offset_at(tz, start);
    let mut subs = vec![observance(
        &Change {
            at: start,
            from: offset,
            to: offset,
            dst,
            name,
        },
        None,
    )];
    let mut runs: Vec<Run> = Vec::new();
    for c in changes(tz, start, end) {
        match runs.iter_mut().find(|r| r.takes(&c)) {
            Some(run) => {
                let day = c.onset().date();
                run.nths.retain(|n| nths(day).contains(n));
                run.changes.push(c);
            }
            None => runs.push(Run {
                nths: nths(c.onset().date()),
                changes: vec![c],
            }),
        }
    }
    let last_year = from_year + SCAN_YEARS - 1;
    for run in runs {
        let (Some(first), Some(last)) = (run.changes.first(), run.changes.last()) else {
            continue;
        };
        if run.changes.len() == 1 {
            subs.push(observance(first, None));
            continue;
        }
        let day = first.onset().date();
        let by_day = match run.nths.contains(&Nth::Last) {
            true => "-1".to_string(),
            false => match run.nths.first() {
                Some(Nth::Nth(n)) => n.to_string(),
                _ => "-1".to_string(),
            },
        };
        let mut rule = format!(
            "FREQ=YEARLY;BYMONTH={};BYDAY={by_day}{}",
            day.month(),
            WEEKDAYS[day.weekday().num_days_from_monday() as usize]
        );
        if last.onset().year() < last_year {
            rule.push_str(&format!(";UNTIL={}", format_utc(&last.at)));
        }
        subs.push(observance(first, Some(rule)));
    }
    VTimezone {
        tzid: tz.name().into(),
        raw_subs: subs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vtimezone::{ResolvedTz, local_to_utc_offset_seconds, resolve};

    /// Every hour of the years after the first, the written zone gives the
    /// offset the zone database gives.
    fn agrees_with_the_database(name: &str, from_year: i32) {
        let tz: Tz = name.parse().expect("iana");
        let block = vtimezone_for(tz, from_year);
        let rt = resolve(name, std::slice::from_ref(&block)).expect("resolves");
        assert!(matches!(rt, ResolvedTz::Custom(_)), "the block is read");
        let mut t = Utc
            .with_ymd_and_hms(from_year + 1, 1, 1, 0, 0, 0)
            .single()
            .expect("start");
        let end = t + Duration::days(366 * 20);
        while t < end {
            let want = offset_at(tz, t).0;
            let local = t.naive_utc() + Duration::seconds(i64::from(want));
            let got = local_to_utc_offset_seconds(&rt, local).expect("offset");
            // in the hour that happens twice the first reading wins; it is
            // still the same wall-clock time
            let earliest = tz
                .from_local_datetime(&local)
                .earliest()
                .map(|d| d.offset().fix().local_minus_utc());
            assert_eq!(Some(got), earliest, "{name} at {t}");
            t += Duration::hours(1);
        }
    }

    #[test]
    fn los_angeles_reads_back_as_the_database_for_twenty_years() {
        agrees_with_the_database("America/Los_Angeles", 2025);
    }

    #[test]
    fn berlin_and_its_last_sundays_read_back_as_the_database() {
        agrees_with_the_database("Europe/Berlin", 2025);
    }

    #[test]
    fn sydney_in_the_south_reads_back_as_the_database() {
        agrees_with_the_database("Australia/Sydney", 2025);
    }

    #[test]
    fn tokyo_has_one_offset_and_no_rule() {
        let block = vtimezone_for("Asia/Tokyo".parse().expect("iana"), 2025);
        assert_eq!(block.raw_subs.len(), 1);
        agrees_with_the_database("Asia/Tokyo", 2025);
    }

    #[test]
    fn los_angeles_is_written_as_its_current_rules() {
        let block = vtimezone_for("America/Los_Angeles".parse().expect("iana"), 2025);
        let rules: Vec<&str> = block
            .raw_subs
            .iter()
            .flat_map(|s| s.properties.iter())
            .filter(|p| p.name == "RRULE")
            .map(|p| p.value.as_str())
            .collect();
        assert_eq!(
            rules,
            vec![
                "FREQ=YEARLY;BYMONTH=3;BYDAY=2SU",
                "FREQ=YEARLY;BYMONTH=11;BYDAY=1SU"
            ]
        );
    }
}

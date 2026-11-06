//! The occurrences of a recurring event (RFC 5545 §3.8.5).
//!
//! A rule repeats a **wall-clock** time in the event's own zone: "every
//! Thursday at 22:00 in Santa Clara" is 05:00 UTC in October and 06:00
//! UTC in November. So the rule is expanded in local time and each
//! occurrence is resolved through the zone separately — never by adding
//! a week to the first occurrence's instant.
//!
//! Expansion is always bounded: by the window asked for, by
//! [`MAX_OCCURRENCES`] results, and by [`MAX_STEPS`] steps of the rule.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::vtimezone::{ResolvedTz, resolve};
use crate::{CalDateTime, EventStatus, Method, ParsedInvite, VTimezone};

mod clock;
mod rule;
mod write;

pub use clock::organiser_clock;
use clock::{moment, moment_at};
pub use rule::Rule;
use rule::split_until;
pub use write::Head;

/// At most this many occurrences come back from one expansion.
pub const MAX_OCCURRENCES: usize = 500;

/// At most this many steps of a rule are walked, matched or not.
pub const MAX_STEPS: usize = 100_000;

/// Every zone is within this much of UTC, so a local-time bound widened by
/// it cannot miss an occurrence.
const WIDEST_OFFSET: i64 = 14 * 3600;

/// How far ahead [`Series::next_after`] looks.
const NEXT_HORIZON_DAYS: i64 = 5 * 366;

/// When an occurrence is: an instant, or for an all-day event a date,
/// which has no instant (see [`crate::instants`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Moment {
    /// A timed occurrence.
    Instant(DateTime<Utc>),
    /// An all-day occurrence; as an end, the day after its last.
    Date(NaiveDate),
}

impl Moment {
    /// The form a stored calendar row uses: RFC 3339, or `YYYY-MM-DD`.
    pub fn to_column(&self) -> String {
        match self {
            Moment::Instant(t) => t.to_rfc3339(),
            Moment::Date(d) => d.format("%Y-%m-%d").to_string(),
        }
    }

    /// The instant used to order and window it; a date counts from its
    /// UTC midnight.
    fn ordering_instant(&self) -> DateTime<Utc> {
        match self {
            Moment::Instant(t) => *t,
            Moment::Date(d) => d.and_time(chrono::NaiveTime::MIN).and_utc(),
        }
    }

    fn plus(&self, secs: i64) -> Moment {
        match self {
            Moment::Instant(t) => Moment::Instant(*t + Duration::seconds(secs)),
            Moment::Date(d) => Moment::Date(*d + Duration::days((secs / 86_400).max(1))),
        }
    }
}

/// One occurrence of a series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// When it starts.
    pub start: Moment,
    /// When it ends (exclusive).
    pub end: Moment,
    /// The occurrence of the rule it is, which an override replaces —
    /// equal to `start` unless the organiser moved it.
    pub recurrence_id: Moment,
}

/// One occurrence the organiser changed or cancelled (a `VEVENT` with a
/// `RECURRENCE-ID`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    /// The occurrence of the rule it replaces.
    pub recurrence_id: CalDateTime,
    /// Its own start.
    pub dtstart: CalDateTime,
    /// Its own length, in seconds.
    pub length_secs: i64,
    /// Cancelled rather than moved.
    pub cancelled: bool,
}

impl Override {
    /// The override an instance invitation describes; `None` when it is
    /// not an instance. A `CANCEL`, or `STATUS:CANCELLED`, cancels it.
    pub fn of(instance: &ParsedInvite) -> Option<Override> {
        Some(Override {
            recurrence_id: instance.recurrence_id.clone()?,
            dtstart: instance.dtstart.clone(),
            length_secs: length_of(instance),
            cancelled: instance.method == Method::Cancel
                || instance.status == Some(EventStatus::Cancelled),
        })
    }
}

/// A recurring event: what its occurrences are computed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    /// The first occurrence's start, in the organiser's zone.
    pub dtstart: CalDateTime,
    /// Every occurrence's length, in seconds.
    pub length_secs: i64,
    /// The raw `RRULE`, when there is one.
    pub rrule: Option<String>,
    /// `EXDATE`s.
    pub exdate: Vec<CalDateTime>,
    /// `RDATE`s.
    pub rdate: Vec<CalDateTime>,
    /// The `VTIMEZONE` blocks the times above refer to.
    pub vtimezones: Vec<VTimezone>,
    /// Occurrences the organiser changed or cancelled.
    #[serde(default)]
    pub overrides: Vec<Override>,
}

/// What a series' times are in.
pub(crate) enum Zone {
    Utc,
    Tz(ResolvedTz),
    AllDay,
}

impl Series {
    /// The series a master invitation describes. `None` for a single event,
    /// for one occurrence of a series (it has a `RECURRENCE-ID`), and for a
    /// floating start, which has no instants to repeat.
    pub fn of(invite: &ParsedInvite) -> Option<Series> {
        if invite.recurrence_id.is_some() || (invite.rrule.is_none() && invite.rdate.is_empty()) {
            return None;
        }
        if matches!(invite.dtstart, CalDateTime::Floating(_)) {
            return None;
        }
        Some(Series {
            dtstart: invite.dtstart.clone(),
            length_secs: length_of(invite),
            rrule: invite.rrule.clone(),
            exdate: invite.exdate.clone(),
            rdate: invite.rdate.clone(),
            vtimezones: invite.vtimezones.clone(),
            overrides: Vec::new(),
        })
    }

    /// Add or replace the override for one occurrence.
    pub fn attach(&mut self, ov: Override) {
        self.overrides
            .retain(|o| o.recurrence_id != ov.recurrence_id);
        self.overrides.push(ov);
    }

    /// Occurrences overlapping `[from, to)`, in start order, at most
    /// [`MAX_OCCURRENCES`].
    pub fn occurrences(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Occurrence> {
        let Some(zone) = self.zone() else {
            return Vec::new();
        };
        let replaced: Vec<Moment> = self
            .overrides
            .iter()
            .filter_map(|o| moment(&o.recurrence_id, &zone, &self.vtimezones))
            .collect();
        let excluded: Vec<Moment> = self
            .exdate
            .iter()
            .filter_map(|d| moment(d, &zone, &self.vtimezones))
            .collect();
        let overlaps = |start: &Moment, end: &Moment| {
            let (s, e) = window_of(start, end);
            s < to && e > from
        };
        let lower = from - Duration::seconds(self.length_secs.max(0) + WIDEST_OFFSET);
        let upper = to + Duration::seconds(WIDEST_OFFSET);
        let ruled = self.rule_starts(&zone, lower, upper);
        let added = self
            .rdate
            .iter()
            .filter_map(|d| moment(d, &zone, &self.vtimezones));
        let mut out: Vec<Occurrence> = ruled
            .into_iter()
            .chain(added)
            .filter(|m| !excluded.contains(m) && !replaced.contains(m))
            .map(|m| Occurrence {
                start: m,
                end: m.plus(self.length_secs),
                recurrence_id: m,
            })
            .filter(|o| overlaps(&o.start, &o.end))
            .take(MAX_OCCURRENCES)
            .collect();
        for ov in self.overrides.iter().filter(|o| !o.cancelled) {
            let (Some(start), Some(rid)) = (
                moment(&ov.dtstart, &zone, &self.vtimezones),
                moment(&ov.recurrence_id, &zone, &self.vtimezones),
            ) else {
                continue;
            };
            let end = start.plus(ov.length_secs);
            if overlaps(&start, &end) {
                out.push(Occurrence {
                    start,
                    end,
                    recurrence_id: rid,
                });
            }
        }
        out.sort_by_key(|o| (o.start.ordering_instant(), o.recurrence_id));
        out.dedup_by_key(|o| o.recurrence_id);
        out.truncate(MAX_OCCURRENCES);
        out
    }

    /// The first occurrence that has not ended by `now` — the one in
    /// progress, or the next. `None` when the series is over.
    pub fn next_after(&self, now: DateTime<Utc>) -> Option<Occurrence> {
        let horizon = now + Duration::days(NEXT_HORIZON_DAYS);
        self.occurrences(now, horizon).into_iter().next()
    }

    pub(crate) fn zone(&self) -> Option<Zone> {
        match &self.dtstart {
            CalDateTime::Utc(_) => Some(Zone::Utc),
            CalDateTime::Date(_) => Some(Zone::AllDay),
            CalDateTime::Zoned { tz_name, .. } => resolve(tz_name, &self.vtimezones).map(Zone::Tz),
            CalDateTime::Floating(_) => None,
        }
    }

    /// The starts the rule (and `DTSTART`, always the first occurrence)
    /// gives between two instants, in order.
    fn rule_starts(&self, zone: &Zone, lower: DateTime<Utc>, upper: DateTime<Utc>) -> Vec<Moment> {
        let Some(first_local) = local_of(&self.dtstart) else {
            return Vec::new();
        };
        let first = moment(&self.dtstart, zone, &self.vtimezones);
        let mut out: Vec<Moment> = first.into_iter().collect();
        let Some(raw) = self.rrule.as_deref() else {
            return out;
        };
        let (rest, until) = split_until(raw);
        let source = format!(
            "DTSTART:{}Z\nRRULE:{rest}",
            first_local.format("%Y%m%dT%H%M%S")
        );
        let Ok(set) = source.parse::<rrule::RRuleSet>() else {
            return out;
        };
        // local wall-clock bounds, wide enough for any zone
        let lower_local = lower.naive_utc() - Duration::seconds(WIDEST_OFFSET);
        let upper_local = upper.naive_utc() + Duration::seconds(WIDEST_OFFSET);
        for local in (&set)
            .into_iter()
            .take(MAX_STEPS)
            .map(|d| d.naive_utc())
            .take_while(|l| *l < upper_local)
        {
            if local < lower_local {
                continue;
            }
            let Some(m) = moment_at(local, zone) else {
                continue;
            };
            if until.as_ref().is_some_and(|u| u.is_before(local, &m)) {
                break;
            }
            out.push(m);
            if out.len() > MAX_OCCURRENCES {
                break;
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

fn length_of(invite: &ParsedInvite) -> i64 {
    if let Some((first, after_last)) = crate::instants::all_day_of(invite) {
        return (after_last - first).num_days() * 86_400;
    }
    match crate::instants::instants_of(invite) {
        (Some(s), Some(e)) => (e - s).num_seconds().max(0),
        _ => 3600,
    }
}

fn local_of(dt: &CalDateTime) -> Option<NaiveDateTime> {
    match dt {
        CalDateTime::Utc(t) => Some(t.naive_utc()),
        CalDateTime::Zoned { local, .. } => Some(*local),
        CalDateTime::Date(d) => Some(d.and_time(chrono::NaiveTime::MIN)),
        CalDateTime::Floating(_) => None,
    }
}

/// An occurrence's span as instants, for overlap with a window. An
/// all-day occurrence is widened by every zone's offset, as the
/// calendar's reader does: the client places it exactly.
fn window_of(start: &Moment, end: &Moment) -> (DateTime<Utc>, DateTime<Utc>) {
    match (start, end) {
        (Moment::Instant(s), Moment::Instant(e)) => (*s, *e),
        _ => (
            start.ordering_instant() - Duration::seconds(WIDEST_OFFSET),
            end.ordering_instant() + Duration::seconds(WIDEST_OFFSET),
        ),
    }
}

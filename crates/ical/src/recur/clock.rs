//! Wall-clock times in a series' zone, and the instants they are.

use chrono::{DateTime, Duration, NaiveDateTime, Utc};

use super::{Moment, Zone};
use crate::vtimezone::{ResolvedTz, local_to_utc_offset_seconds, resolve};
use crate::{CalDateTime, VTimezone};

/// A date-time of the series (an EXDATE, an RDATE, a RECURRENCE-ID) as a
/// moment. One that names its own zone is resolved through it. A bare date
/// in a timed series names no instant, and none is guessed for it.
pub(super) fn moment(dt: &CalDateTime, zone: &Zone, zones: &[VTimezone]) -> Option<Moment> {
    match (dt, zone) {
        (CalDateTime::Date(d), Zone::AllDay) => Some(Moment::Date(*d)),
        (CalDateTime::Date(_), _) => None,
        (CalDateTime::Utc(t), Zone::AllDay) => Some(Moment::Date(t.date_naive())),
        (CalDateTime::Zoned { local, .. }, Zone::AllDay) => Some(Moment::Date(local.date())),
        (CalDateTime::Utc(t), _) => Some(Moment::Instant(*t)),
        (CalDateTime::Zoned { tz_name, local }, _) => {
            let rt = resolve(tz_name, zones)?;
            Some(Moment::Instant(local_to_utc(&rt, *local)?))
        }
        (CalDateTime::Floating(local), z) => moment_at(*local, z),
    }
}

pub(super) fn moment_at(local: NaiveDateTime, zone: &Zone) -> Option<Moment> {
    match zone {
        Zone::Utc => Some(Moment::Instant(local.and_utc())),
        Zone::AllDay => Some(Moment::Date(local.date())),
        Zone::Tz(rt) => local_to_utc(rt, local).map(Moment::Instant),
    }
}

/// A wall-clock time in a zone as an instant, gaps and repeats read as
/// [`local_to_utc_offset_seconds`] reads them.
fn local_to_utc(rt: &ResolvedTz, local: NaiveDateTime) -> Option<DateTime<Utc>> {
    let offset = local_to_utc_offset_seconds(rt, local)?;
    Some((local - Duration::seconds(offset as i64)).and_utc())
}

/// The organiser's wall-clock time at `at`, and their UTC offset then, in
/// the zone `dt` names. `None` for a time that names no zone.
pub fn organiser_clock(
    dt: &CalDateTime,
    zones: &[VTimezone],
    at: DateTime<Utc>,
) -> Option<(NaiveDateTime, i32)> {
    match dt {
        CalDateTime::Utc(_) => Some((at.naive_utc(), 0)),
        CalDateTime::Zoned { tz_name, .. } => {
            let rt = resolve(tz_name, zones)?;
            let local = utc_to_local(&rt, at.naive_utc());
            Some((local, (local - at.naive_utc()).num_seconds() as i32))
        }
        CalDateTime::Date(_) | CalDateTime::Floating(_) => None,
    }
}

/// An instant as the zone's wall-clock time. Exact, including in the hour
/// that happens twice: each transition is placed on the UTC line by the
/// offset in force before it.
pub(super) fn utc_to_local(rt: &ResolvedTz, utc: NaiveDateTime) -> NaiveDateTime {
    match rt {
        ResolvedTz::Iana(tz) => {
            use chrono::TimeZone;
            tz.from_utc_datetime(&utc).naive_local()
        }
        ResolvedTz::Custom(transitions) => {
            let mut offset: Option<i32> = None;
            for t in transitions {
                let before = offset.unwrap_or(t.utc_offset_seconds);
                if t.effective_from - Duration::seconds(before as i64) > utc {
                    break;
                }
                offset = Some(t.utc_offset_seconds);
            }
            utc + Duration::seconds(offset.unwrap_or(0) as i64)
        }
    }
}

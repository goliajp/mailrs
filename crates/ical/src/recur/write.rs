//! A series written back out as iCalendar text, for a calendar client.
//!
//! Every date-time is written in the form `DTSTART` has — with the same
//! `TZID`, in UTC, or as a date — because clients match `EXDATE` and
//! `RECURRENCE-ID` against the rule's occurrences by value. A zone the
//! organiser defined is written as they defined it; a zone known only by
//! name is written under its IANA name with a `VTIMEZONE` generated from
//! the zone database: a client that knows the name uses its own copy of
//! the zone, and one that does not still has the definition RFC 5545
//! requires. A Windows name is not used for that, since only the
//! organiser's own definition makes it readable.
//!
//! A cancelled occurrence is written as an `EXDATE`, which every client
//! reads as "not happening".

use chrono::{NaiveDateTime, NaiveTime};

use super::clock::{local_to_utc, moment, utc_to_local};
use super::rule::{Until, split_until};
use super::{Moment, Series};
use crate::serialize::{
    escape_text, format_date, format_naive, format_utc, push_line, write_vtimezone,
};
use crate::vtimezone::{ResolvedTz, resolve};
use crate::{CalDateTime, VTimezone};

/// What a series' text says besides its times.
#[derive(Debug, Clone, Copy)]
pub struct Head<'a> {
    /// `UID`.
    pub uid: &'a str,
    /// `SUMMARY`.
    pub summary: &'a str,
    /// The organiser's address.
    pub organizer: Option<&'a str>,
    /// `STATUS`, as written.
    pub status: Option<&'a str>,
    /// `SEQUENCE`.
    pub sequence: i32,
    /// `DTSTAMP`, in the UTC form (`20261029T000000Z`).
    pub dtstamp: &'a str,
}

/// The form every date-time of the series is written in.
enum Form {
    Utc,
    Date,
    Zoned {
        tzid: String,
        rt: ResolvedTz,
        block: VTimezone,
    },
}

impl Form {
    fn of(series: &Series) -> Option<Form> {
        match &series.dtstart {
            CalDateTime::Utc(_) => Some(Form::Utc),
            CalDateTime::Date(_) => Some(Form::Date),
            CalDateTime::Floating(_) => None,
            CalDateTime::Zoned { tz_name, local } => {
                let rt = resolve(tz_name, &series.vtimezones)?;
                let (tzid, block) = match &rt {
                    ResolvedTz::Custom(_) => {
                        let own = series
                            .vtimezones
                            .iter()
                            .find(|b| b.tzid.eq_ignore_ascii_case(tz_name.trim()))?;
                        (own.tzid.to_string(), own.clone())
                    }
                    ResolvedTz::Iana(tz) => {
                        use chrono::Datelike;
                        let block = crate::zone_block::vtimezone_for(*tz, local.year() - 1);
                        (tz.name().to_string(), block)
                    }
                };
                Some(Form::Zoned { tzid, rt, block })
            }
        }
    }

    /// `m` as what follows a property's name.
    fn value(&self, m: &Moment) -> Option<String> {
        match (self, m) {
            (Form::Utc, Moment::Instant(t)) => Some(format!(":{}", format_utc(t))),
            (Form::Date, Moment::Date(d)) => Some(format!(";VALUE=DATE:{}", format_date(d))),
            (Form::Zoned { tzid, rt, .. }, Moment::Instant(t)) => Some(format!(
                ";TZID={}:{}",
                param(tzid),
                format_naive(&utc_to_local(rt, t.naive_utc()))
            )),
            _ => None,
        }
    }

    /// `UNTIL` in the form RFC 5545 §3.3.10 requires: a date when
    /// `DTSTART` is a date, otherwise UTC. Inclusive either way, as the
    /// series reads it.
    fn until(&self, until: &Until) -> Option<String> {
        let end_of = |d: chrono::NaiveDate| Some(d.and_time(NaiveTime::from_hms_opt(23, 59, 59)?));
        let utc_of =
            |rt: &ResolvedTz, local: NaiveDateTime| local_to_utc(rt, local).map(|t| format_utc(&t));
        match (self, until) {
            (Form::Date, Until::Instant(t)) => Some(format_date(&t.date_naive())),
            (Form::Date, Until::Local(l)) => Some(format_date(&l.date())),
            (Form::Date, Until::Date(d)) => Some(format_date(d)),
            (_, Until::Instant(t)) => Some(format_utc(t)),
            (Form::Utc, Until::Local(l)) => Some(format_utc(&l.and_utc())),
            (Form::Utc, Until::Date(d)) => Some(format_utc(&end_of(*d)?.and_utc())),
            (Form::Zoned { rt, .. }, Until::Local(l)) => utc_of(rt, *l),
            (Form::Zoned { rt, .. }, Until::Date(d)) => utc_of(rt, end_of(*d)?),
        }
    }
}

/// A `TZID` value, quoted when it holds a character that ends a parameter.
fn param(v: &str) -> String {
    match v.contains([':', ';', ',']) {
        true => format!("\"{}\"", v.replace('"', "")),
        false => v.to_string(),
    }
}

impl Series {
    /// The series as a `VCALENDAR`: the zone its times are in, the series
    /// itself, and one `VEVENT` per moved occurrence. `None` when its start
    /// names a zone nothing resolves.
    pub fn to_calendar(&self, head: &Head<'_>) -> Option<String> {
        let form = Form::of(self)?;
        let zone = self.zone()?;
        let at = |dt: &CalDateTime| moment(dt, &zone, &self.vtimezones);
        let first = at(&self.dtstart)?;
        let mut out = String::new();
        push_line(&mut out, "BEGIN:VCALENDAR");
        push_line(&mut out, "VERSION:2.0");
        push_line(&mut out, "PRODID:-//mailrs//calendar//EN");
        if let Form::Zoned { block, .. } = &form {
            write_vtimezone(&mut out, block);
        }

        let mut master = vec![
            format!("DTSTART{}", form.value(&first)?),
            format!("DTEND{}", form.value(&first.plus(self.length_secs))?),
        ];
        if let Some(raw) = &self.rrule {
            let (rest, until) = split_until(raw);
            match until {
                Some(u) => master.push(format!("RRULE:{rest};UNTIL={}", form.until(&u)?)),
                None => master.push(format!("RRULE:{rest}")),
            }
        }
        let mut excluded: Vec<Moment> = self.exdate.iter().filter_map(at).collect();
        excluded.extend(
            self.overrides
                .iter()
                .filter(|o| o.cancelled)
                .filter_map(|o| at(&o.recurrence_id)),
        );
        excluded.sort();
        excluded.dedup();
        master.extend(
            excluded
                .iter()
                .filter_map(|m| form.value(m))
                .map(|v| format!("EXDATE{v}")),
        );
        let mut added: Vec<Moment> = self.rdate.iter().filter_map(at).collect();
        added.sort();
        added.dedup();
        master.extend(
            added
                .iter()
                .filter_map(|m| form.value(m))
                .map(|v| format!("RDATE{v}")),
        );
        write_event(&mut out, head, &master);

        let mut moved: Vec<(Moment, Moment, i64)> = self
            .overrides
            .iter()
            .filter(|o| !o.cancelled)
            .filter_map(|o| Some((at(&o.recurrence_id)?, at(&o.dtstart)?, o.length_secs)))
            .collect();
        moved.sort_by_key(|(rid, _, _)| *rid);
        for (rid, start, length) in moved {
            let (Some(r), Some(s), Some(e)) = (
                form.value(&rid),
                form.value(&start),
                form.value(&start.plus(length)),
            ) else {
                continue;
            };
            let lines = [
                format!("RECURRENCE-ID{r}"),
                format!("DTSTART{s}"),
                format!("DTEND{e}"),
            ];
            write_event(&mut out, head, &lines);
        }
        push_line(&mut out, "END:VCALENDAR");
        Some(out)
    }
}

fn write_event(out: &mut String, head: &Head<'_>, times: &[String]) {
    push_line(out, "BEGIN:VEVENT");
    push_line(out, &format!("UID:{}", escape_text(head.uid)));
    push_line(out, &format!("SEQUENCE:{}", head.sequence));
    push_line(out, &format!("DTSTAMP:{}", head.dtstamp));
    for line in times {
        push_line(out, line);
    }
    push_line(out, &format!("SUMMARY:{}", escape_text(head.summary)));
    if let Some(o) = head.organizer {
        push_line(out, &format!("ORGANIZER:mailto:{o}"));
    }
    if let Some(s) = head.status {
        push_line(out, &format!("STATUS:{s}"));
    }
    push_line(out, "END:VEVENT");
}

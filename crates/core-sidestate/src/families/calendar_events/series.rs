//! Recurring events on a stored row.
//!
//! A row that repeats keeps its series — rule, exclusions, additions,
//! the organiser's `VTIMEZONE`, and the occurrences the organiser later
//! moved or cancelled — in [`StoredEvent::recurrence`], as JSON of
//! [`Series`]. Its `dtstart` / `dtend` stay the first occurrence, which is
//! what the index is scored by; readers expand the series into the
//! occurrences of the window they show.

use std::io;

use chrono::{DateTime, Utc};
use mailrs_ical::recur::{Occurrence, Override, Series, organiser_clock};
use mailrs_ical::{CalDateTime, ParsedInvite};

use super::{StoredEvent, event_key, from_flat, put};

/// The `recurrence` column of a master event; `None` for one that does
/// not repeat.
pub fn column_of(invite: &ParsedInvite) -> Option<String> {
    Series::of(invite).and_then(|s| serde_json::to_string(&s).ok())
}

/// The series a row repeats by, if it repeats.
pub fn series_of(ev: &StoredEvent) -> Option<Series> {
    serde_json::from_str(ev.recurrence.as_deref()?).ok()
}

/// The occurrences of `ev` overlapping `[from, to)`, or `None` when it
/// does not repeat.
pub fn occurrences(
    ev: &StoredEvent,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Option<Vec<Occurrence>> {
    Some(series_of(ev)?.occurrences(from, to))
}

/// A master event filed over a stored one keeps the occurrences the
/// organiser changed on their own: Exchange sends each of those as a
/// mail of its own, and a later update of the series does not repeat them.
pub(super) fn carry_overrides(stored: &StoredEvent, row: &mut StoredEvent) {
    let (Some(old), Some(mut new)) = (series_of(stored), series_of(row)) else {
        return;
    };
    for ov in old.overrides {
        if !new
            .overrides
            .iter()
            .any(|o| o.recurrence_id == ov.recurrence_id)
        {
            new.attach(ov);
        }
    }
    row.recurrence = serde_json::to_string(&new).ok();
}

/// File one changed or cancelled occurrence into the stored series it
/// belongs to. `None` when `instance` is not one occurrence or there is no
/// stored series for it — the caller files it as an event of its own then.
pub(super) fn file_instance(
    conn: &mut kevy_client::Connection,
    user: &str,
    uid: &str,
    instance: &ParsedInvite,
) -> io::Result<Option<bool>> {
    let Some(ov) = Override::of(instance) else {
        return Ok(None);
    };
    let flat = conn.hgetall(event_key(user, uid).as_bytes())?;
    if flat.is_empty() {
        return Ok(None);
    }
    let mut stored = from_flat(uid, &flat);
    let Some(mut series) = series_of(&stored) else {
        return Ok(None);
    };
    series.attach(ov);
    stored.recurrence = serde_json::to_string(&series).ok();
    // the text a CalDAV client stored no longer says what the row does
    stored.ics = None;
    put(conn, user, &stored)?;
    Ok(Some(true))
}

/// Add what a reader needs about a recurring invitation to its payload,
/// at the moment it is read — the next occurrence moves on every week,
/// so it cannot be computed once at delivery.
///
/// - `organiser_zone`: the IANA zone the organiser wrote the time in,
///   when the `TZID` names one (directly or as a Windows name), so a
///   client formats the instant in it with its own zone database.
/// - `recurrence`: the rule, read for showing in words, or `null`.
/// - `next_occurrence`: `{start, end, organiser_local, organiser_offset_secs}`
///   for the first occurrence that has not ended, or `null` when the
///   series is over. `start` / `end` are RFC 3339 instants, or dates for an
///   all-day series.
///
/// An invitation for one occurrence (`RECURRENCE-ID`) is not a series:
/// it gets `organiser_zone` and nothing else.
pub fn annotate(payload: &mut serde_json::Value, now: DateTime<Utc>) {
    let Ok(invite) = serde_json::from_value::<ParsedInvite>(payload.clone()) else {
        return;
    };
    let Some(obj) = payload.as_object_mut() else {
        return;
    };
    let zone = match &invite.dtstart {
        CalDateTime::Zoned { tz_name, .. } => mailrs_ical::vtimezone::iana_name(tz_name),
        _ => None,
    };
    obj.insert("organiser_zone".into(), zone.into());
    let Some(series) = Series::of(&invite) else {
        return;
    };
    obj.insert(
        "recurrence".into(),
        serde_json::to_value(series.rule()).unwrap_or_default(),
    );
    let next = series.next_after(now).map(|o| {
        let clock = match o.start {
            mailrs_ical::recur::Moment::Instant(t) => {
                organiser_clock(&series.dtstart, &series.vtimezones, t)
            }
            mailrs_ical::recur::Moment::Date(_) => None,
        };
        serde_json::json!({
            "start": o.start.to_column(),
            "end": o.end.to_column(),
            "organiser_local": clock.map(|(l, _)| l.format("%Y-%m-%dT%H:%M:%S").to_string()),
            "organiser_offset_secs": clock.map(|(_, off)| off),
        })
    });
    obj.insert("next_occurrence".into(), next.into());
}

/// Every event of a document, one per `UID`: a series' changed and
/// cancelled occurrences, which come as `VEVENT`s of their own with the
/// same `UID` and a `RECURRENCE-ID`, are folded into it as overrides.
/// Each comes back with its `recurrence` column.
///
/// An occurrence whose series is not in the document is kept as an event
/// of its own, as is the first of several events sharing a `UID` with no
/// series among them.
pub fn fold_document(events: Vec<ParsedInvite>) -> Vec<(ParsedInvite, Option<String>)> {
    let mut masters: Vec<(ParsedInvite, Option<Series>)> = Vec::new();
    let mut instances: Vec<ParsedInvite> = Vec::new();
    for ev in events {
        match ev.recurrence_id.is_some() {
            true => instances.push(ev),
            false => {
                let series = Series::of(&ev);
                masters.push((ev, series));
            }
        }
    }
    for inst in instances {
        let master = masters.iter_mut().find(|(m, _)| m.uid == inst.uid);
        match (master, Override::of(&inst)) {
            (Some((_, Some(series))), Some(ov)) => series.attach(ov),
            (Some(_), _) => {}
            (None, _) => masters.push((inst, None)),
        }
    }
    masters
        .into_iter()
        .map(|(ev, series)| {
            let column = series.and_then(|s| serde_json::to_string(&s).ok());
            (ev, column)
        })
        .collect()
}

/// [`fold_document`] for iCalendar text: each `VEVENT` read with the
/// document's `VTIMEZONE`s, unreadable ones skipped.
pub fn fold_text(text: &str) -> Vec<(ParsedInvite, Option<String>)> {
    let parsed = mailrs_ical::parse::split_vevents(text)
        .iter()
        .filter_map(|doc| mailrs_ical::parse_invite(doc.as_bytes()).ok())
        .collect();
    fold_document(parsed)
}

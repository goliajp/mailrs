//! Type conversions between `mailrs_ical` enums and the
//! string/JSON forms used in the PG schema.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use serde_json::json;

use mailrs_ical::vtimezone::{local_to_utc_offset_seconds, resolve};
use mailrs_ical::{
    Attendee as IcalAttendee, CalDateTime, EventStatus, PartStat, Person, Role as IcalRole,
    VTimezone,
};

pub(crate) fn caldatetime_to_utc(
    dt: &CalDateTime,
    vtimezones: &[VTimezone],
) -> Option<DateTime<Utc>> {
    match dt {
        CalDateTime::Utc(d) => Some(*d),
        CalDateTime::Floating(n) => Some(n.and_utc()),
        CalDateTime::Zoned { tz_name, local } => {
            let resolved = resolve(tz_name, vtimezones)?;
            let off = local_to_utc_offset_seconds(&resolved, *local)?;
            let utc = local
                .checked_sub_signed(chrono::Duration::seconds(off as i64))?;
            Some(utc.and_utc())
        }
        CalDateTime::Date(d) => Some(naive_date_to_utc_midnight(*d)),
    }
}


pub(super) fn naive_date_to_utc_midnight(d: NaiveDate) -> DateTime<Utc> {
    let zero = NaiveTime::from_hms_opt(0, 0, 0).expect("00:00:00 always valid");
    NaiveDateTime::new(d, zero).and_utc()
}


pub(super) fn person_to_string(p: &Person) -> String {
    p.email.clone()
}


pub(super) fn attendee_to_json(a: &IcalAttendee) -> serde_json::Value {
    json!({
        "email": a.email,
        "cn": a.cn,
        "partstat": partstat_str(a.partstat),
        "role": role_str(a.role),
        "rsvp": a.rsvp,
    })
}


pub(super) fn partstat_str(p: PartStat) -> &'static str {
    match p {
        PartStat::NeedsAction => "NEEDS-ACTION",
        PartStat::Accepted => "ACCEPTED",
        PartStat::Declined => "DECLINED",
        PartStat::Tentative => "TENTATIVE",
        PartStat::Delegated => "DELEGATED",
        PartStat::Completed => "COMPLETED",
        PartStat::InProcess => "IN-PROCESS",
    }
}


pub(super) fn role_str(r: IcalRole) -> &'static str {
    match r {
        IcalRole::Chair => "CHAIR",
        IcalRole::ReqParticipant => "REQ-PARTICIPANT",
        IcalRole::OptParticipant => "OPT-PARTICIPANT",
        IcalRole::NonParticipant => "NON-PARTICIPANT",
    }
}


pub(super) fn event_status_str(s: EventStatus) -> &'static str {
    match s {
        EventStatus::Confirmed => "CONFIRMED",
        EventStatus::Tentative => "TENTATIVE",
        EventStatus::Cancelled => "CANCELLED",
    }
}


pub(super) fn method_str(m: mailrs_ical::Method) -> &'static str {
    use mailrs_ical::Method::*;
    match m {
        Request => "REQUEST",
        Reply => "REPLY",
        Cancel => "CANCEL",
        Update => "UPDATE",
        Counter => "COUNTER",
        Refresh => "REFRESH",
        Add => "ADD",
        Publish => "PUBLISH",
        DeclineCounter => "DECLINECOUNTER",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_ical::{Method, ParsedInvite, VTimezone};
    use chrono::TimeZone;

    fn make_parsed(uid: &str, dtstart: CalDateTime) -> ParsedInvite {
        ParsedInvite {
            method: Method::Request,
            uid: uid.into(),
            sequence: 0,
            dtstamp: Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap(),
            dtstart,
            dtend: None,
            duration: None,
            organizer: None,
            attendees: vec![],
            rrule: None,
            exdate: vec![],
            rdate: vec![],
            recurrence_id: None,
            status: None,
            summary: "test".into(),
            location: None,
            description: None,
            vtimezones: vec![],
        }
    }

    #[test]
    fn utc_caldatetime_passes_through() {
        let d = Utc.with_ymd_and_hms(2026, 5, 1, 14, 0, 0).unwrap();
        let p = make_parsed("a", CalDateTime::Utc(d));
        let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
        assert_eq!(out, d);
    }

    #[test]
    fn floating_caldatetime_treated_as_utc() {
        let n = chrono::NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap();
        let p = make_parsed("b", CalDateTime::Floating(n));
        let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
        assert_eq!(out, n.and_utc());
    }

    #[test]
    fn zoned_caldatetime_with_iana_resolves() {
        // 2026-05-01 14:00 in Asia/Tokyo = 2026-05-01 05:00 UTC.
        let n = chrono::NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap();
        let dt = CalDateTime::Zoned {
            tz_name: "Asia/Tokyo".into(),
            local: n,
        };
        let p = make_parsed("c", dt);
        let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
        assert_eq!(out, Utc.with_ymd_and_hms(2026, 5, 1, 5, 0, 0).unwrap());
    }

    #[test]
    fn date_only_caldatetime_uses_midnight_utc() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let p = make_parsed("d", CalDateTime::Date(d));
        let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
        assert_eq!(out, Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap());
    }

    #[test]
    fn zoned_unknown_tz_returns_none() {
        let n = chrono::NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap();
        let dt = CalDateTime::Zoned {
            tz_name: "Made/Up_Zone_That_Doesn't_Exist".into(),
            local: n,
        };
        let p = make_parsed("e", dt);
        assert!(caldatetime_to_utc(&p.dtstart, &p.vtimezones).is_none());
    }
}


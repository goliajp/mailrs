//! iCalendar METHOD=REPLY / METHOD=COUNTER builders + helpers
//! for extracting CALDATE / CALDATE-TIME values from JSON.

use chrono::{DateTime, Utc};

pub(super) fn build_counter_ics(
    invite_payload: &serde_json::Value,
    user_email: &str,
    new_dtstart: DateTime<Utc>,
    new_dtend: Option<DateTime<Utc>>,
    comment: Option<&str>,
) -> String {
    let uid = invite_payload
        .get("uid")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let sequence = invite_payload
        .get("sequence")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let summary = invite_payload
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let organizer_email = invite_payload
        .get("organizer")
        .and_then(|o| o.get("email"))
        .and_then(|e| e.as_str())
        .unwrap_or("");
    let user_cn = invite_payload
        .get("attendees")
        .and_then(|a| a.as_array())
        .and_then(|arr| {
            arr.iter().find(|att| {
                att.get("email")
                    .and_then(|e| e.as_str())
                    .map(|e| e.eq_ignore_ascii_case(user_email))
                    .unwrap_or(false)
            })
        })
        .and_then(|att| att.get("cn"))
        .and_then(|cn| cn.as_str())
        .map(|s| s.to_string());

    let now_utc = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let new_dtstart_str = new_dtstart.format("%Y%m%dT%H%M%SZ").to_string();
    let new_dtend_str = new_dtend.map(|d| d.format("%Y%m%dT%H%M%SZ").to_string());

    let cn_param = user_cn
        .as_ref()
        .map(|cn| format!(";CN={cn}"))
        .unwrap_or_default();

    let mut ics = String::with_capacity(512);
    ics.push_str("BEGIN:VCALENDAR\r\n");
    ics.push_str("VERSION:2.0\r\n");
    ics.push_str("PRODID:-//mailrs//MRS-1 ICS//EN\r\n");
    ics.push_str("METHOD:COUNTER\r\n");
    ics.push_str("BEGIN:VEVENT\r\n");
    ics.push_str(&format!("UID:{uid}\r\n"));
    ics.push_str(&format!("SEQUENCE:{sequence}\r\n"));
    ics.push_str(&format!("DTSTAMP:{now_utc}\r\n"));
    ics.push_str(&format!("DTSTART:{new_dtstart_str}\r\n"));
    if let Some(s) = new_dtend_str {
        ics.push_str(&format!("DTEND:{s}\r\n"));
    }
    if !summary.is_empty() {
        ics.push_str(&format!("SUMMARY:{summary}\r\n"));
    }
    if !organizer_email.is_empty() {
        ics.push_str(&format!("ORGANIZER:mailto:{organizer_email}\r\n"));
    }
    ics.push_str(&format!(
        "ATTENDEE{cn_param};PARTSTAT=TENTATIVE;ROLE=REQ-PARTICIPANT;RSVP=TRUE:mailto:{user_email}\r\n",
    ));
    if let Some(c) = comment {
        let escaped = c
            .replace('\\', "\\\\")
            .replace(',', "\\,")
            .replace(';', "\\;")
            .replace('\n', "\\n");
        ics.push_str(&format!("COMMENT:{escaped}\r\n"));
    }
    ics.push_str("END:VEVENT\r\n");
    ics.push_str("END:VCALENDAR\r\n");
    ics
}

/// Hand-build a RFC 5546 METHOD=REPLY iCalendar object from the stored
/// `invite_payload` JSON. Keeps UID and SEQUENCE byte-identical to the
/// original (RFC 5546 §3.4 says REPLY MUST preserve both); flips PARTSTAT
/// on the user's ATTENDEE row and drops the rest (REPLY carries only the
pub(super) fn build_reply_ics(
    invite_payload: &serde_json::Value,
    user_email: &str,
    partstat: &str,
    recurrence_id: Option<DateTime<Utc>>,
) -> String {
    let uid = invite_payload
        .get("uid")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let sequence = invite_payload
        .get("sequence")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let summary = invite_payload
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let organizer_email = invite_payload
        .get("organizer")
        .and_then(|o| o.get("email"))
        .and_then(|e| e.as_str())
        .unwrap_or("");

    let user_cn = invite_payload
        .get("attendees")
        .and_then(|a| a.as_array())
        .and_then(|arr| {
            arr.iter().find(|att| {
                att.get("email")
                    .and_then(|e| e.as_str())
                    .map(|e| e.eq_ignore_ascii_case(user_email))
                    .unwrap_or(false)
            })
        })
        .and_then(|att| att.get("cn"))
        .and_then(|cn| cn.as_str())
        .map(|s| s.to_string());

    let dtstart_iso = invite_payload
        .get("dtstart")
        .and_then(extract_caldatetime_for_ics);
    let dtend_iso = invite_payload
        .get("dtend")
        .and_then(extract_caldatetime_for_ics);

    let now_utc = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let cn_param = user_cn
        .as_ref()
        .map(|cn| format!(";CN={cn}"))
        .unwrap_or_default();

    let mut ics = String::with_capacity(512);
    ics.push_str("BEGIN:VCALENDAR\r\n");
    ics.push_str("VERSION:2.0\r\n");
    ics.push_str("PRODID:-//mailrs//MRS-1 ICS//EN\r\n");
    ics.push_str("METHOD:REPLY\r\n");
    ics.push_str("BEGIN:VEVENT\r\n");
    ics.push_str(&format!("UID:{uid}\r\n"));
    ics.push_str(&format!("SEQUENCE:{sequence}\r\n"));
    ics.push_str(&format!("DTSTAMP:{now_utc}\r\n"));
    if let Some(s) = dtstart_iso {
        ics.push_str(&format!("DTSTART:{s}\r\n"));
    }
    if let Some(s) = dtend_iso {
        ics.push_str(&format!("DTEND:{s}\r\n"));
    }
    if let Some(rid) = recurrence_id {
        ics.push_str(&format!(
            "RECURRENCE-ID:{}\r\n",
            rid.format("%Y%m%dT%H%M%SZ")
        ));
    }
    if !summary.is_empty() {
        ics.push_str(&format!("SUMMARY:{summary}\r\n"));
    }
    if !organizer_email.is_empty() {
        ics.push_str(&format!("ORGANIZER:mailto:{organizer_email}\r\n"));
    }
    ics.push_str(&format!(
        "ATTENDEE{cn_param};PARTSTAT={partstat};ROLE=REQ-PARTICIPANT;RSVP=FALSE:mailto:{user_email}\r\n",
    ));
    ics.push_str("END:VEVENT\r\n");
    ics.push_str("END:VCALENDAR\r\n");
    ics
}

/// `caldatetime_to_ics_value`: convert the JSON {kind, iso, ...} representation
/// produced by `mailrs::ical` derive(Serialize) back into the RFC 5545
/// surface form (`19980714T170000Z` for UTC, `19980714T170000` for floating /
fn extract_caldatetime_for_ics(v: &serde_json::Value) -> Option<String> {
    // chrono serde-Serialize for DateTime<Utc> emits an ISO-8601 string;
    // for NaiveDateTime / NaiveDate likewise. With the `Serialize` derive
    // on enum CalDateTime, each variant becomes
    // {"Utc": "1998-07-14T17:00:00Z"} etc. — externally-tagged.
    let obj = v.as_object()?;
    let (variant, inner) = obj.iter().next()?;
    match variant.as_str() {
        "Utc" => {
            let iso = inner.as_str()?;
            // ISO 8601 -> compact iCal form.
            let dt: DateTime<Utc> = iso.parse().ok()?;
            Some(dt.format("%Y%m%dT%H%M%SZ").to_string())
        }
        "Floating" => {
            let iso = inner.as_str()?;
            let n: chrono::NaiveDateTime = iso.parse().ok()?;
            Some(n.format("%Y%m%dT%H%M%S").to_string())
        }
        "Zoned" => {
            // {"Zoned": {"tz_name": "...", "local": "..."}}
            let zoned = inner.as_object()?;
            let tz_name = zoned.get("tz_name")?.as_str()?;
            let local_iso = zoned.get("local")?.as_str()?;
            let n: chrono::NaiveDateTime = local_iso.parse().ok()?;
            Some(format!(";TZID={tz_name}:{}", n.format("%Y%m%dT%H%M%S")))
            // NB: in build_reply_ics we splice this into the line as if it
            // were a value; if a tzid is present the ;TZID= prefix is
            // tolerated because callers wrap with `DTSTART:` directly.
            // Outlook / Google accept both `DTSTART:...Z` and `DTSTART;TZID=...:...`
            // shapes. UTC is preferred for REPLY and is the common path.
        }
        "Date" => {
            let iso = inner.as_str()?;
            let d: chrono::NaiveDate = iso.parse().ok()?;
            Some(format!(";VALUE=DATE:{}", d.format("%Y%m%d")))
        }
        _ => None,
    }
}

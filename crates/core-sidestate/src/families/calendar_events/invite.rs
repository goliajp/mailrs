//! The event an invitation in the mail describes, and the reader's own
//! answer to it.
//!
//! Two writers file invitations — delivery, and an answer given to an
//! invitation that arrived before delivery filed them — so the row is
//! built in one place and both go through [`file_invite`].

use std::io;

use super::{StoredEvent, event_key, from_flat, get, put};

/// The answers an attendee can give (RFC 5545 `PARTSTAT`), as stored.
pub const ANSWERS: [&str; 3] = ["ACCEPTED", "TENTATIVE", "DECLINED"];

/// The row an invitation describes.
///
/// `invite` is the typed invite as ingest stores it beside the message:
/// `mailrs_ical::ParsedInvite` serialized, with `dtstart_utc` /
/// `dtend_utc` resolved to instants. A `CANCEL` marks the event cancelled
/// rather than removing it.
pub fn from_invite(
    uid: &str,
    method: &str,
    sequence: i32,
    invite: &serde_json::Value,
) -> StoredEvent {
    let text = |field: &str| {
        invite
            .get(field)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let status = match method.eq_ignore_ascii_case("CANCEL") {
        true => Some("CANCELLED".to_string()),
        false => text("status"),
    };
    StoredEvent {
        uid: uid.to_string(),
        summary: text("summary").unwrap_or_default(),
        dtstart: text("dtstart_utc"),
        dtend: text("dtend_utc"),
        organizer: invite
            .get("organizer")
            .and_then(|o| o.get("email"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        status,
        source: format!("mail:{uid}"),
        sequence,
        ics: None,
        partstat: None,
        message_id: None,
    }
}

/// File `row` unless the stored copy is newer. `false` when it was older
/// and left alone.
///
/// A lower `SEQUENCE` never overwrites a higher: re-reading old copies of a
/// re-sent meeting, which is what a backfill does, cannot walk the event
/// backwards. The reader's answer is not one of the columns [`put`] writes,
/// so an update from the organiser keeps it.
pub fn file_invite(
    conn: &mut kevy_client::Connection,
    user: &str,
    row: &StoredEvent,
) -> io::Result<bool> {
    let flat = conn.hgetall(event_key(user, &row.uid).as_bytes())?;
    if !flat.is_empty() && from_flat(&row.uid, &flat).sequence > row.sequence {
        return Ok(false);
    }
    put(conn, user, row)?;
    Ok(true)
}

/// Record the reader's answer on the event. `false` when there is no such
/// event to answer.
pub fn set_partstat(
    conn: &mut kevy_client::Connection,
    user: &str,
    uid: &str,
    partstat: &str,
) -> io::Result<bool> {
    if get(conn, user, uid)?.is_none() {
        return Ok(false);
    }
    conn.hset(
        event_key(user, uid).as_bytes(),
        &[(b"partstat" as &[u8], partstat.as_bytes())],
    )?;
    Ok(true)
}

/// Link the event to the mail it came in. `false` when there is no such
/// event.
pub fn set_message_id(
    conn: &mut kevy_client::Connection,
    user: &str,
    uid: &str,
    message_id: &str,
) -> io::Result<bool> {
    if get(conn, user, uid)?.is_none() {
        return Ok(false);
    }
    conn.hset(
        event_key(user, uid).as_bytes(),
        &[(b"message_id" as &[u8], message_id.as_bytes())],
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> kevy_client::Connection {
        kevy_client::Connection::connect(&format!("mem://calendar-invite-{name}")).expect("kevy")
    }

    fn invite(summary: &str) -> serde_json::Value {
        serde_json::json!({
            "summary": summary,
            "dtstart_utc": "2026-11-05T01:00:00+00:00",
            "dtend_utc": "2026-11-05T02:00:00+00:00",
            "organizer": { "email": "chair@example.com" },
            "status": "CONFIRMED",
        })
    }

    /// The link to the mail survives what rewrites the row: a later copy
    /// of the invitation without one, and a CalDAV client's PUT.
    #[test]
    fn the_mail_link_survives_an_update_and_a_caldav_copy() {
        let mut c = conn("mail-link");
        let mut row = from_invite("u2", "REQUEST", 0, &invite("Review"));
        row.message_id = Some("inv@chair.example".into());
        assert!(file_invite(&mut c, "a@x", &row).unwrap());
        let newer = from_invite("u2", "REQUEST", 1, &invite("Review, moved"));
        assert!(file_invite(&mut c, "a@x", &newer).unwrap());
        let caldav = StoredEvent {
            source: "caldav".into(),
            ..from_invite("u2", "REQUEST", 2, &invite("Review, mine"))
        };
        crate::families::calendar_events::put(&mut c, "a@x", &caldav).unwrap();
        let ev = get(&mut c, "a@x", "u2").unwrap().unwrap();
        assert_eq!(ev.summary, "Review, mine");
        assert_eq!(ev.message_id.as_deref(), Some("inv@chair.example"));
        assert!(set_message_id(&mut c, "a@x", "u2", "again@chair.example").unwrap());
        assert!(!set_message_id(&mut c, "a@x", "missing", "x@y").unwrap());
        let ev = get(&mut c, "a@x", "u2").unwrap().unwrap();
        assert_eq!(ev.message_id.as_deref(), Some("again@chair.example"));
    }

    #[test]
    fn an_answer_lands_on_the_event() {
        let mut c = conn("answer");
        let row = from_invite("u1", "REQUEST", 0, &invite("Sync"));
        assert!(file_invite(&mut c, "a@x", &row).unwrap());
        assert!(set_partstat(&mut c, "a@x", "u1", "ACCEPTED").unwrap());
        let ev = get(&mut c, "a@x", "u1").unwrap().unwrap();
        assert_eq!(ev.partstat.as_deref(), Some("ACCEPTED"));
        assert_eq!(ev.organizer.as_deref(), Some("chair@example.com"));
        assert_eq!(ev.source, "mail:u1");
    }

    #[test]
    fn there_is_nothing_to_answer_without_an_event() {
        let mut c = conn("missing");
        assert!(!set_partstat(&mut c, "a@x", "nope", "ACCEPTED").unwrap());
        assert!(get(&mut c, "a@x", "nope").unwrap().is_none());
    }

    /// The organiser moves the meeting after the reader accepted: the new
    /// time is filed and the answer stays.
    #[test]
    fn an_update_from_the_organiser_keeps_the_answer() {
        let mut c = conn("update");
        file_invite(
            &mut c,
            "a@x",
            &from_invite("u1", "REQUEST", 0, &invite("Sync")),
        )
        .unwrap();
        set_partstat(&mut c, "a@x", "u1", "ACCEPTED").unwrap();
        let moved = from_invite("u1", "REQUEST", 1, &invite("Sync, moved"));
        assert!(file_invite(&mut c, "a@x", &moved).unwrap());
        let ev = get(&mut c, "a@x", "u1").unwrap().unwrap();
        assert_eq!(ev.summary, "Sync, moved");
        assert_eq!(ev.sequence, 1);
        assert_eq!(ev.partstat.as_deref(), Some("ACCEPTED"));
    }

    #[test]
    fn an_older_copy_does_not_walk_the_event_back() {
        let mut c = conn("older");
        file_invite(
            &mut c,
            "a@x",
            &from_invite("u1", "REQUEST", 2, &invite("New")),
        )
        .unwrap();
        let old = from_invite("u1", "REQUEST", 1, &invite("Old"));
        assert!(!file_invite(&mut c, "a@x", &old).unwrap());
        assert_eq!(get(&mut c, "a@x", "u1").unwrap().unwrap().summary, "New");
    }

    #[test]
    fn a_cancel_is_kept_as_cancelled() {
        let row = from_invite("u1", "Cancel", 3, &invite("Sync"));
        assert_eq!(row.status.as_deref(), Some("CANCELLED"));
    }
}

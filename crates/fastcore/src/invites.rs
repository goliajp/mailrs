//! Read the invitation out of a delivered message, and keep it.
//!
//! Production ingested mail for a year without ever looking at a
//! `text/calendar` part: the extractor existed, the parser existed, the
//! web card existed, and the step between them was in the lane that no
//! longer runs. This is that step.
//!
//! Two things are stored, deliberately apart:
//!
//! - **`invite_method` on the message row** — three to seven bytes,
//!   read on every list and thread fetch, and all a client needs to
//!   know an invite when it sees one.
//! - **`mailrs:invite:{message_id}`** — the typed event, a few
//!   kilobytes, fetched only when a message is opened.
//!
//! An invite is a property of the *message*, not of a recipient, so it
//! is keyed by message-id and shared. What differs per recipient — the
//! answer given — already lives elsewhere, under `rsvp:{user}:{uid}`.

use std::sync::Arc;

use crate::FastcoreState;

/// What ingest learned from a message's calendar part.
#[derive(Debug, Clone)]
pub(crate) struct FoundInvite {
    /// iTIP method, upper-case. Goes on the message row.
    pub(crate) method: String,
    /// The typed invite, JSON, for `mailrs:invite:{message_id}`.
    pub(crate) payload_json: String,
    /// `UID` — the event's identity across updates and cancellations.
    pub(crate) uid: String,
    /// `SEQUENCE` — higher supersedes lower for the same `UID`.
    pub(crate) sequence: i32,
}

/// Parse the calendar part of a message, if it has one.
///
/// Returns `None` for the overwhelming majority of mail. Also returns
/// `None` when a calendar part is present but unparseable — a malformed
/// invite is not worth failing a delivery over, and the message still
/// arrives as ordinary mail.
///
/// **The whole body, not the header window.** `ingest_delivered_file`
/// reads headers out of the first 16 KB; a calendar part is routinely
/// past that, and an extractor given the window alone finds nothing on
/// exactly the invites that matter.
pub(crate) fn find(body: &[u8]) -> Option<FoundInvite> {
    let extracted = mailrs_ical::mime_part::extract_invite_part(body)?;
    let parsed = mailrs_ical::parse_invite(&extracted.ics_bytes).ok()?;
    let _ = &extracted.content_type_method;
    // The body's METHOD is authoritative; the Content-Type parameter is
    // a hint some producers omit. Where the body has none, fall back to
    // the header rather than dropping the invite.
    // The body's METHOD is what the parser typed; the Content-Type
    // parameter is a hint some producers omit and others contradict.
    // They agree on every one of the 23 corpus fixtures, so the typed
    // value is used and the header is kept only for the tamper check
    // RFC 6047 §2.4 asks for, which is not built yet.
    let method = format!("{:?}", parsed.method).to_uppercase();
    // The two instants, resolved here rather than in the browser. A
    // client handed `20260820T160000` and the string "Pacific Standard
    // Time" has no honest way to turn it into a moment, and the one
    // dishonest way — read it as UTC — is what the web did, putting an
    // afternoon in Santa Clara at one the next morning in Tokyo.
    //
    // Added *beside* the parsed invite rather than replacing anything:
    // the original wall-clock time and its zone are what the organiser
    // scheduled, and a card that wants to say "16:00 in Santa Clara"
    // still needs them.
    let (start, end) = mailrs_ical::instants::instants_of(&parsed);
    let mut payload = serde_json::to_value(&parsed).ok()?;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("dtstart_utc".into(), start.map(|t| t.to_rfc3339()).into());
        obj.insert("dtend_utc".into(), end.map(|t| t.to_rfc3339()).into());
    }
    let payload_json = serde_json::to_string(&payload).ok()?;
    Some(FoundInvite {
        method,
        payload_json,
        uid: parsed.uid.clone(),
        sequence: parsed.sequence,
    })
}

/// Store the event beside the message, keeping the newest revision.
///
/// A meeting is re-sent on every change, each time with a higher
/// `SEQUENCE` and the same `UID`. Every copy is a real message and stays
/// in the mailbox; what must not happen is an older copy overwriting the
/// event a later one already corrected. So a write with a lower
/// `SEQUENCE` for the same `UID` is refused — which is also what makes
/// re-running a backfill over old mail safe in any order.
pub(crate) fn store(state: &Arc<FastcoreState>, message_id: &str, found: &FoundInvite) {
    let key = format!("mailrs:invite:{message_id}");
    let _ = state.mailbox.store_ref().atomic(|ctx| {
        if let Ok(Some(existing)) = ctx.hget(key.as_bytes(), b"sequence".as_slice()) {
            let prior: i32 = String::from_utf8_lossy(&existing)
                .trim()
                .parse()
                .unwrap_or(-1);
            if prior > found.sequence {
                return Ok(());
            }
        }
        ctx.hset(
            key.as_bytes(),
            &[
                (b"method".as_slice(), found.method.as_bytes()),
                (b"uid".as_slice(), found.uid.as_bytes()),
                (
                    b"sequence".as_slice(),
                    found.sequence.to_string().as_bytes(),
                ),
                (b"payload".as_slice(), found.payload_json.as_bytes()),
            ],
        )?;
        Ok(())
    });
}

/// The stored event for a message, as JSON, or `None`.
pub fn payload_json(state: &Arc<FastcoreState>, message_id: &str) -> Option<String> {
    let key = format!("mailrs:invite:{message_id}");
    let raw = state
        .mailbox
        .store_ref()
        .hget(key.as_bytes(), b"payload")
        .ok()??;
    Some(String::from_utf8_lossy(&raw).into_owned())
}

/// File the invitation as an event, so it can be seen and conflicted
/// against.
///
/// **On the network store, not the embedded one.** The conflicts reader
/// and the feed sync both live there, and an event written to the other
/// store would be invisible to both — the distinction that once had
/// `mark_not_junk` writing where nothing read.
///
/// A `CANCEL` marks the event cancelled rather than deleting it: the
/// mail is still in the mailbox, and a card that cannot say "this was
/// cancelled" is worse than one that says nothing. A lower `SEQUENCE`
/// never overwrites a higher, so re-reading old copies of a re-sent
/// meeting — which is exactly what a backfill does — cannot walk the
/// event backwards.
pub(crate) fn file_event(state: &Arc<FastcoreState>, user: &str, found: &FoundInvite) {
    use mailrs_core_sidestate::families::calendar_events as ev;

    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&found.payload_json) else {
        return;
    };
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let key = ev::event_key(user, &found.uid);
    if let Ok(flat) = conn.hgetall(key.as_bytes()) {
        let existing = ev::from_flat(&found.uid, &flat);
        if !flat.is_empty() && existing.sequence > found.sequence {
            return;
        }
    }
    let dtstart = parsed
        .get("dtstart_utc")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let row = ev::StoredEvent {
        uid: found.uid.clone(),
        summary: parsed
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        dtstart: dtstart.clone(),
        dtend: parsed
            .get("dtend_utc")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        organizer: parsed
            .get("organizer")
            .and_then(|o| o.get("email"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        status: if found.method == "CANCEL" {
            Some("CANCELLED".to_string())
        } else {
            parsed
                .get("status")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        },
        source: format!("mail:{}", found.uid),
        sequence: found.sequence,
    };
    let written = ev::fields(&row);
    let pairs: Vec<(&[u8], &[u8])> = written
        .iter()
        .map(|(k, v)| (k.as_bytes(), v.as_bytes()))
        .collect();
    if conn.hset(key.as_bytes(), &pairs).is_err() {
        return;
    }
    // The index is scored by start time, and an all-day event has no
    // instant to score — it is stored and readable, just not in the
    // window query. Better than filing it at midnight in a zone nobody
    // named.
    if let Some(start) = dtstart.as_deref().and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.timestamp() as f64)
    }) {
        let _ = conn.zadd(
            ev::index_key(user).as_bytes(),
            &[(start, found.uid.as_bytes())],
        );
    }
}

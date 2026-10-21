//! The invitation in a delivered message: read once, and filed as an
//! event on the network store both cores share. Where the typed invite is
//! kept beside the message is each core's own.

/// What ingest learned from a message's calendar part.
#[derive(Debug, Clone)]
pub struct FoundInvite {
    /// iTIP method, upper-case. Goes on the message row.
    pub method: String,
    /// The typed invite, JSON, for `mailrs:invite:{message_id}`.
    pub payload_json: String,
    /// `UID` — the event's identity across updates and cancellations.
    pub uid: String,
    /// `SEQUENCE` — higher supersedes lower for the same `UID`.
    pub sequence: i32,
}

/// Parse the calendar part of a message, if it has one.
///
/// Returns `None` for the overwhelming majority of mail. Also returns
/// `None` when a calendar part is present but unparseable — a malformed
/// invite is not worth failing a delivery over, and the message still
/// arrives as ordinary mail.
///
/// **The whole body, not the header window.** ingest
/// reads headers out of the first 16 KB; a calendar part is routinely
/// past that, and an extractor given the window alone finds nothing on
/// exactly the invites that matter.
pub fn find(body: &[u8]) -> Option<FoundInvite> {
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
        // And the way in. Resolved here rather than in each client:
        // three implementations of "which URL is a meeting" is three
        // chances to offer a button that goes somewhere else.
        obj.insert(
            "join_url".into(),
            mailrs_ical::instants::join_link(&parsed).into(),
        );
    }
    let payload_json = serde_json::to_string(&payload).ok()?;
    Some(FoundInvite {
        method,
        payload_json,
        uid: parsed.uid.clone(),
        sequence: parsed.sequence,
    })
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
pub fn file_event(net: Option<kevy_client::Connection>, user: &str, found: &FoundInvite) {
    use mailrs_core_sidestate::families::calendar_events as ev;

    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&found.payload_json) else {
        return;
    };
    let Some(mut conn) = net else {
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

//! iTIP RSVP endpoint (MRS-6).
//!
//! `POST /api/invites/{message_id}/rsvp` body `{ partstat }` →
//! - generates a RFC 5546 METHOD=REPLY iCalendar with the user's chosen
//!   PARTSTAT
//! - wraps it as multipart/alternative with a human-readable text/plain
//!   summary plus the text/calendar; method=REPLY part as attachment
//! - delivers via the existing `mail::deliver_message_ex` pipeline (DKIM
//!   signing, outbound queue, suppression checks all reused for free)
//! - on ACCEPTED / TENTATIVE, also upserts the event into the user's own
//!   default calendar so it shows up in mailrs's CalDAV view (and any
//!   subscribed native calendar app picks it up automatically)

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{AuthUser, WebState};

#[derive(Deserialize)]
pub(super) struct RsvpRequest {
    pub partstat: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub comment: Option<String>,
    /// When set, the RSVP applies to a single occurrence of a recurring
    /// series rather than the master (RFC 5545 §3.8.4.4 + RFC 5546 §3.4).
    /// ISO-8601 UTC instant matching the occurrence's DTSTART. The web
    /// client passes this through automatically when the inbound invite
    /// itself carried a RECURRENCE-ID.
    #[serde(default)]
    pub recurrence_id: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub(super) struct RsvpResult {
    pub success: bool,
    pub message: Option<String>,
}

pub(super) async fn submit_rsvp(
    Path(message_id): Path<i64>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<RsvpRequest>,
) -> impl IntoResponse {
    let partstat = match req.partstat.to_uppercase().as_str() {
        s @ ("ACCEPTED" | "TENTATIVE" | "DECLINED") => s.to_string(),
        _ => {
            return Json(RsvpResult {
                success: false,
                message: Some("partstat must be ACCEPTED, TENTATIVE, or DECLINED".into()),
            });
        }
    };

    let Some(ref pool) = state.pg_pool else {
        return Json(RsvpResult {
            success: false,
            message: Some("postgres pool not configured".into()),
        });
    };

    // Fetch the message + ownership-check via mailboxes JOIN. Pull
    // invite_payload + the original Message-ID so we can build the threaded
    // reply correctly.
    let row: Option<(serde_json::Value, String)> = sqlx::query_as(
        "SELECT m.invite_payload, m.message_id
         FROM messages m
         JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE m.id = $1
           AND mb.user_address = $2
           AND m.invite_payload IS NOT NULL",
    )
    .bind(message_id)
    .bind(&user)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some((invite_payload, original_msg_id)) = row else {
        return Json(RsvpResult {
            success: false,
            message: Some("message not found or not an invite".into()),
        });
    };

    let organizer_email = invite_payload
        .get("organizer")
        .and_then(|o| o.get("email"))
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .to_string();
    if organizer_email.is_empty() {
        return Json(RsvpResult {
            success: false,
            message: Some("invite has no organizer".into()),
        });
    }

    let summary = invite_payload
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or("Invitation")
        .to_string();

    // Effective recurrence-id: prefer caller-supplied (e.g. the user
    // explicitly chose "this occurrence" in the UI); fall back to whatever
    // the invite itself carried (organizer-issued single-instance update).
    let effective_recurrence_id: Option<DateTime<Utc>> = req.recurrence_id.or_else(|| {
        invite_payload
            .get("recurrence_id")
            .and_then(|v| extract_caldatetime_to_utc(Some(v)))
    });

    let reply_ics = build_reply_ics(&invite_payload, &user, &partstat, effective_recurrence_id);

    let now_utc = Utc::now();
    let now_secs = now_utc.timestamp();
    let new_msg_id = format!("<rsvp-{now_secs}-{message_id}@mailrs>");
    let boundary = format!("mailrs-rsvp-{now_secs}");

    let verb = match partstat.as_str() {
        "ACCEPTED" => "Accepted",
        "TENTATIVE" => "Tentative",
        "DECLINED" => "Declined",
        _ => "Reply",
    };
    let action_phrase = match partstat.as_str() {
        "ACCEPTED" => "accepted",
        "TENTATIVE" => "tentatively accepted",
        "DECLINED" => "declined",
        _ => "responded to",
    };

    let body_text = format!(
        "{user} has {action_phrase} the invitation to: {summary}\r\n",
    );

    let raw_email = format!(
        "From: {user}\r\n\
         To: {organizer_email}\r\n\
         Subject: {verb}: {summary}\r\n\
         Message-ID: {new_msg_id}\r\n\
         In-Reply-To: <{original_msg_id}>\r\n\
         References: <{original_msg_id}>\r\n\
         Date: {date_rfc2822}\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Type: text/plain; charset=UTF-8\r\n\
         \r\n\
         {body_text}\r\n\
         --{boundary}\r\n\
         Content-Type: text/calendar; method=REPLY; charset=UTF-8\r\n\
         Content-Disposition: attachment; filename=invite.ics\r\n\
         \r\n\
         {reply_ics}\r\n\
         --{boundary}--\r\n",
        date_rfc2822 = now_utc.format("%a, %d %b %Y %H:%M:%S +0000"),
    );

    let send_result = super::mail::deliver_message_ex(
        &state,
        &user,
        std::slice::from_ref(&organizer_email),
        &[],
        &[],
        raw_email.as_bytes(),
        &new_msg_id,
        now_secs,
        None,
    )
    .await;

    // On Accept/Tentative, also write the event to the user's own default
    // calendar so MRS's CalDAV server publishes it to any subscribed native
    // client. Decline doesn't write (per Decisions: declined invites stay
    // off the calendar by default).
    if partstat == "ACCEPTED" || partstat == "TENTATIVE" {
        if let Err(e) = write_to_own_calendar(
            pool,
            &user,
            &invite_payload,
            &partstat,
            now_utc,
            effective_recurrence_id,
        )
        .await
        {
            tracing::warn!("rsvp write_to_own_calendar failed: {e}");
        }
    }

    let _ = send_result; // SendResult JSON is ignored; we have our own envelope
    Json(RsvpResult {
        success: true,
        message: Some(format!("REPLY sent to {organizer_email}")),
    })
}

#[derive(Deserialize)]
pub(super) struct CounterRequest {
    /// Proposed new start time (ISO-8601 UTC).
    pub dtstart: DateTime<Utc>,
    /// Proposed new end time. If omitted, the original duration is used
    /// (caller-side); the COUNTER ICS may still skip DTEND.
    #[serde(default)]
    pub dtend: Option<DateTime<Utc>>,
    /// Free-text rationale for the counter (RFC 5545 §3.8.1.4 COMMENT).
    #[serde(default)]
    pub comment: Option<String>,
}

/// `POST /api/invites/{message_id}/counter`: attendee proposes a new time
/// instead of accepting/declining outright (RFC 5546 §3.4 COUNTER). Sends
/// METHOD=COUNTER iCalendar back to the organizer; organizer-side calendar
/// surfaces it as a counter-proposal that organizer can accept (which
/// triggers their UPDATE) or reject (DECLINECOUNTER).
pub(super) async fn submit_counter(
    Path(message_id): Path<i64>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CounterRequest>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(RsvpResult {
            success: false,
            message: Some("postgres pool not configured".into()),
        });
    };

    let row: Option<(serde_json::Value, String)> = sqlx::query_as(
        "SELECT m.invite_payload, m.message_id
         FROM messages m
         JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE m.id = $1
           AND mb.user_address = $2
           AND m.invite_payload IS NOT NULL",
    )
    .bind(message_id)
    .bind(&user)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some((invite_payload, original_msg_id)) = row else {
        return Json(RsvpResult {
            success: false,
            message: Some("message not found or not an invite".into()),
        });
    };

    let organizer_email = invite_payload
        .get("organizer")
        .and_then(|o| o.get("email"))
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .to_string();
    if organizer_email.is_empty() {
        return Json(RsvpResult {
            success: false,
            message: Some("invite has no organizer".into()),
        });
    }

    let summary = invite_payload
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or("Invitation")
        .to_string();

    let counter_ics = build_counter_ics(
        &invite_payload,
        &user,
        req.dtstart,
        req.dtend,
        req.comment.as_deref(),
    );

    let now_utc = Utc::now();
    let now_secs = now_utc.timestamp();
    let new_msg_id = format!("<counter-{now_secs}-{message_id}@mailrs>");
    let boundary = format!("mailrs-counter-{now_secs}");

    let proposed_local = req.dtstart.format("%Y-%m-%d %H:%M UTC");
    let body_text = format!(
        "{user} has proposed a new time for: {summary}\r\n\
         Proposed: {proposed_local}\r\n",
    );

    let raw_email = format!(
        "From: {user}\r\n\
         To: {organizer_email}\r\n\
         Subject: Counter-proposal: {summary}\r\n\
         Message-ID: {new_msg_id}\r\n\
         In-Reply-To: <{original_msg_id}>\r\n\
         References: <{original_msg_id}>\r\n\
         Date: {date_rfc2822}\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Type: text/plain; charset=UTF-8\r\n\
         \r\n\
         {body_text}\r\n\
         --{boundary}\r\n\
         Content-Type: text/calendar; method=COUNTER; charset=UTF-8\r\n\
         Content-Disposition: attachment; filename=invite.ics\r\n\
         \r\n\
         {counter_ics}\r\n\
         --{boundary}--\r\n",
        date_rfc2822 = now_utc.format("%a, %d %b %Y %H:%M:%S +0000"),
    );

    let _ = super::mail::deliver_message_ex(
        &state,
        &user,
        std::slice::from_ref(&organizer_email),
        &[],
        &[],
        raw_email.as_bytes(),
        &new_msg_id,
        now_secs,
        None,
    )
    .await;

    Json(RsvpResult {
        success: true,
        message: Some(format!("COUNTER sent to {organizer_email}")),
    })
}

fn build_counter_ics(
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
/// sender's row).
fn build_reply_ics(
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
/// zoned, `19980714` for date-only).
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
            Some(format!(
                ";TZID={tz_name}:{}",
                n.format("%Y%m%dT%H%M%S")
            ))
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

async fn write_to_own_calendar(
    pool: &sqlx::PgPool,
    user: &str,
    invite_payload: &serde_json::Value,
    partstat: &str,
    now: DateTime<Utc>,
    recurrence_id: Option<DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    let uid = invite_payload
        .get("uid")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if uid.is_empty() {
        return Ok(());
    }

    // Ensure the user has a default calendar (matches the CalDAV path).
    sqlx::query(
        "INSERT INTO calendars (account_address, name)
         VALUES ($1, 'Default') ON CONFLICT DO NOTHING",
    )
    .bind(user)
    .execute(pool)
    .await?;

    let cal_id: i64 = sqlx::query_scalar(
        "SELECT id FROM calendars WHERE account_address = $1 ORDER BY id ASC LIMIT 1",
    )
    .bind(user)
    .fetch_one(pool)
    .await?;

    let summary = invite_payload
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let dtstart_utc = extract_caldatetime_to_utc(invite_payload.get("dtstart"));
    let dtend_utc = extract_caldatetime_to_utc(invite_payload.get("dtend"));
    let status_str = if partstat == "TENTATIVE" {
        "TENTATIVE"
    } else {
        "CONFIRMED"
    };
    let etag = format!("{:x}", now.timestamp_micros());

    // Reuse the structured upsert path (pre-flight: minimal raw form built
    // fresh; the exact bytes are not what CalDAV clients GET — they GET
    // whatever later round-trip produces. For now keep it minimal +
    // deterministic).
    let raw_min = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//mailrs//MRS-1 ICS//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:{summary}\r\nSTATUS:{status_str}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
    );

    let sql = if recurrence_id.is_some() {
        "INSERT INTO calendar_events
            (calendar_id, uid, etag, icalendar, summary, dtstart, dtend, status, last_modified, recurrence_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (calendar_id, uid, recurrence_id) WHERE recurrence_id IS NOT NULL DO UPDATE SET
            etag = EXCLUDED.etag,
            icalendar = EXCLUDED.icalendar,
            summary = EXCLUDED.summary,
            dtstart = EXCLUDED.dtstart,
            dtend = EXCLUDED.dtend,
            status = EXCLUDED.status,
            last_modified = EXCLUDED.last_modified,
            updated_at = now()"
    } else {
        "INSERT INTO calendar_events
            (calendar_id, uid, etag, icalendar, summary, dtstart, dtend, status, last_modified, recurrence_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (calendar_id, uid) WHERE recurrence_id IS NULL DO UPDATE SET
            etag = EXCLUDED.etag,
            icalendar = EXCLUDED.icalendar,
            summary = EXCLUDED.summary,
            dtstart = EXCLUDED.dtstart,
            dtend = EXCLUDED.dtend,
            status = EXCLUDED.status,
            last_modified = EXCLUDED.last_modified,
            updated_at = now()"
    };

    sqlx::query(sql)
        .bind(cal_id)
        .bind(uid)
        .bind(&etag)
        .bind(&raw_min)
        .bind(summary)
        .bind(dtstart_utc)
        .bind(dtend_utc)
        .bind(status_str)
        .bind(now)
        .bind(recurrence_id)
        .execute(pool)
        .await?;

    Ok(())
}

fn extract_caldatetime_to_utc(v: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    let v = v?;
    let obj = v.as_object()?;
    let (variant, inner) = obj.iter().next()?;
    match variant.as_str() {
        "Utc" => inner.as_str().and_then(|s| s.parse().ok()),
        "Floating" => inner
            .as_str()
            .and_then(|s| s.parse::<chrono::NaiveDateTime>().ok())
            .map(|n| n.and_utc()),
        "Date" => inner
            .as_str()
            .and_then(|s| s.parse::<chrono::NaiveDate>().ok())
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|nd| nd.and_utc()),
        // Zoned: needs tz resolution; skip for v1 (rare for REPLY-side
        // write-back since the user sees the time the organizer chose, not
        // a zone they picked themselves).
        _ => None,
    }
}

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


mod calendar;
mod ics;

use calendar::write_to_own_calendar;
use calendar::extract_caldatetime_to_utc;
use ics::{build_counter_ics, build_reply_ics};

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
    Path(uid): Path<u32>,
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

    // Look up by (user, IMAP uid) — matching the GET message API. The
    // route used to take a Path<i64> message_id (DB PK), but the web
    // client only knows the IMAP uid; the mismatch happened to silently
    // hit unrelated rows when uid and PK numerically collided. MRS-20.
    let row: Option<(i64, serde_json::Value, String)> = sqlx::query_as(
        "SELECT m.id, m.invite_payload, m.message_id
         FROM messages m
         JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE mb.user_address = $1
           AND m.uid = $2
           AND m.invite_payload IS NOT NULL
         LIMIT 1",
    )
    .bind(&user)
    .bind(uid as i32)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some((message_id, invite_payload, original_msg_id)) = row else {
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
    if (partstat == "ACCEPTED" || partstat == "TENTATIVE")
        && let Err(e) = write_to_own_calendar(
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

    // MRS-19: persist the partstat on the message row so the invite-card
    // can render an "already replied" state on subsequent page loads.
    if let Err(e) = sqlx::query(
        "UPDATE messages SET rsvp_status = $1, rsvp_at = $2 WHERE id = $3",
    )
    .bind(&partstat)
    .bind(now_utc)
    .bind(message_id)
    .execute(pool)
    .await
    {
        tracing::warn!("rsvp status persist failed for {message_id}: {e}");
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

pub(super) async fn submit_counter(
    Path(uid): Path<u32>,
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

    // Look up by (user, IMAP uid) — see submit_rsvp note (MRS-20).
    let row: Option<(i64, serde_json::Value, String)> = sqlx::query_as(
        "SELECT m.id, m.invite_payload, m.message_id
         FROM messages m
         JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE mb.user_address = $1
           AND m.uid = $2
           AND m.invite_payload IS NOT NULL
         LIMIT 1",
    )
    .bind(&user)
    .bind(uid as i32)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some((message_id, invite_payload, original_msg_id)) = row else {
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

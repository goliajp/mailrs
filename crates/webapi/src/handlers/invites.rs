//! `/api/invites/{message_id}/rsvp` + `/api/invites/{message_id}/counter`
//! — iTIP RSVP and counter-proposal endpoints.
//!
//! Storage layout on network kevy:
//!
//!   rsvp:<user>:<message_id>       hash { partstat, comment, recurrence_id, replied_at }
//!   rsvp_counter:<user>:<message_id> hash { start, end, comment, sent_at }
//!
//! **The reply is sent.** Until 2026-08-20 this wrote the choice to
//! kevy and stopped there — the paragraph here claimed it enqueued a
//! REPLY, and the word `outbound` appeared nowhere else in the file. So
//! pressing Accept told nobody: the organiser waited, the card said
//! "accepted" from local state, and nothing anywhere reported a
//! failure. The `rsvp:` row is still written, because it is what the
//! card reads back after a refresh; what is new is the message that
//! leaves.
//!
//! The iTIP body comes from `mailrs_ical::reply` — one attendee, the
//! request's UID and SEQUENCE echoed — because a reply that is subtly
//! wrong fails inside the organiser's client, where nobody here will
//! ever see it.

use axum::Json;
use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use std::sync::Arc;

use axum::extract::State;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
pub struct RsvpRequest {
    pub partstat: String,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub recurrence_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RsvpResponse {
    pub success: bool,
    pub message: Option<String>,
}

/// POST /api/invites/{message_id}/rsvp — record the user's ACCEPT /
/// TENTATIVE / DECLINE response to a calendar invite.
pub async fn submit_rsvp(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(message_id): Path<String>,
    Json(req): Json<RsvpRequest>,
) -> impl IntoResponse {
    let partstat = match req.partstat.to_uppercase().as_str() {
        s @ ("ACCEPTED" | "TENTATIVE" | "DECLINED") => s.to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RsvpResponse {
                    success: false,
                    message: Some("partstat must be ACCEPTED, TENTATIVE, or DECLINED".into()),
                }),
            );
        }
    };

    let key = format!("rsvp:{user}:{message_id}");
    let answer = partstat.clone();
    let comment = req.comment.unwrap_or_default();
    let rec = req.recurrence_id.unwrap_or_default();
    let ts = now_secs().to_string();
    let write = with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[
                (b"partstat" as &[u8], partstat.as_bytes()),
                (b"comment", comment.as_bytes()),
                (b"recurrence_id", rec.as_bytes()),
                (b"replied_at", ts.as_bytes()),
            ],
        )?;
        Ok(())
    });
    if let Err(s) = write {
        return (
            s,
            Json(RsvpResponse {
                success: false,
                message: Some("storage error".into()),
            }),
        );
    }
    match send_reply(&state, &user, &message_id, &answer).await {
        Ok(()) => (
            StatusCode::OK,
            Json(RsvpResponse {
                success: true,
                message: None,
            }),
        ),
        // The choice is recorded either way — a reply that could not be
        // sent is not a reason to forget what the reader chose — but the
        // answer says so rather than reporting success for a message
        // that never left.
        Err(why) => (
            StatusCode::ACCEPTED,
            Json(RsvpResponse {
                success: false,
                message: Some(why),
            }),
        ),
    }
}

/// Build the iTIP REPLY and put it on the outbound queue.
///
/// Errors are strings for the caller to hand back: every one of them is
/// a thing the reader can act on ("this invitation names no organiser")
/// or a thing an operator must see, and swallowing them is how the
/// silence this replaces lasted a year.
async fn send_reply(
    state: &Arc<WebState>,
    user: &str,
    message_uid: &str,
    partstat: &str,
) -> Result<(), String> {
    use base64::Engine as _;

    let uid: u32 = message_uid
        .parse()
        .map_err(|_| "this message has no uid to reply about".to_string())?;
    let answer = mailrs_ical::reply::partstat_from_wire(partstat)
        .ok_or_else(|| format!("{partstat} is not an answer"))?;
    let payload = state
        .core
        .get_invite(user, uid)
        .await
        .map_err(|_| "the invitation is no longer stored".to_string())?;
    let request: mailrs_ical::ParsedInvite = serde_json::from_value(payload)
        .map_err(|e| format!("the stored invitation did not read back: {e}"))?;
    let organizer = mailrs_ical::reply::organizer_of(&request)
        .map(|o| o.email.clone())
        .ok_or_else(|| "this invitation names no organizer to reply to".to_string())?;

    let now = chrono::Utc::now();
    let message_id = format!("<rsvp-{}-{uid}@{}>", now.timestamp(), domain_of(user));
    let email =
        mailrs_ical::reply::reply_message(&request, user, answer, &now.to_rfc2822(), &message_id)
            .map_err(|e| format!("the reply could not be built: {e:?}"))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&email);
    let sender = user.to_string();
    with_kevy(move |c| {
        mailrs_core_sidestate::families::outbound::write_fresh_pending(
            c,
            &mailrs_core_sidestate::families::outbound::FreshPending {
                sender: &sender,
                recipient: &organizer,
                message_data_base64: &b64,
                scheduled_at: None,
                original_sender: None,
                // Generated on the reader's behalf rather than composed,
                // so it does not belong in their Send list.
                send_id: None,
            },
            now.timestamp(),
        )
        .map(|_| ())
    })
    .map_err(|_| "the reply could not be queued".to_string())
}

fn domain_of(address: &str) -> &str {
    address
        .split_once('@')
        .map(|(_, d)| d)
        .unwrap_or("localhost")
}

#[derive(Debug, Deserialize)]
pub struct CounterRequest {
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub comment: Option<String>,
}

/// POST /api/invites/{message_id}/counter — counter-propose a new time.
pub async fn submit_counter(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(message_id): Path<String>,
    Json(req): Json<CounterRequest>,
) -> impl IntoResponse {
    let key = format!("rsvp_counter:{user}:{message_id}");
    let comment = req.comment.unwrap_or_default();
    let ts = now_secs().to_string();
    let start = req.start;
    let end = req.end;
    let write = with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[
                (b"start" as &[u8], start.as_bytes()),
                (b"end", end.as_bytes()),
                (b"comment", comment.as_bytes()),
                (b"sent_at", ts.as_bytes()),
            ],
        )?;
        Ok(())
    });
    match write {
        Ok(_) => (
            StatusCode::OK,
            Json(RsvpResponse {
                success: true,
                message: None,
            }),
        ),
        Err(s) => (
            s,
            Json(RsvpResponse {
                success: false,
                message: Some("storage error".into()),
            }),
        ),
    }
}

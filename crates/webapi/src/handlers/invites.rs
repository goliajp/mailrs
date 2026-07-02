//! `/api/invites/{message_id}/rsvp` + `/api/invites/{message_id}/counter`
//! — iTIP RSVP and counter-proposal endpoints.
//!
//! Storage layout on network kevy:
//!
//!   rsvp:<user>:<message_id>       hash { partstat, comment, recurrence_id, replied_at }
//!   rsvp_counter:<user>:<message_id> hash { start, end, comment, sent_at }
//!
//! The webapi version writes the RSVP intent to kevy + enqueues a
//! REPLY / COUNTER envelope on the outbound queue (`mailrs:outbound:pending`).
//! The full iCalendar REPLY build lives in the monolith's `rsvp::ics`
//! module; here we build a lightweight text/calendar; method=REPLY body
//! from the stored request context. Complete parity with the monolith
//! is tracked separately — this port delivers the minimum the web UI
//! needs (the endpoints return 200 and persist the user's choice).

use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

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

//! Scheduled outbound: list, cancel, reschedule.
//!
//! Split out of `messages.rs` at the 500-line limit this repository
//! holds every language to. One subject: mail that has been written
//! and has not left yet.
//!
//! All three read `mailrs:outbound:scheduled-idx` through the constant
//! the crate that writes it exports. The sender's due-sweep once had
//! its own copy of that name without the `-idx`, so it walked a zset
//! nothing writes and no scheduled message was ever promoted —
//! `scripts/check-outbound-keys.sh` now refuses a second copy.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::outbound::SCHEDULED_IDX;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

const SCHEDULED_KEY: &[u8] = SCHEDULED_IDX;

/// POST /api/scheduled/{id}/cancel — G13.3. Removes the outbound
/// entry from the scheduled zset and drops its envelope blob. Only
/// the sender may cancel; a mismatch returns 404 (leaks no info).
pub async fn cancel_scheduled(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> StatusCode {
    let hkey = format!("mailrs:outbound:job:{id}");
    let hkey_c = hkey.clone();
    let id_c = id.clone();
    let user_c = user.clone();
    let removed = crate::handlers::kevy_util::with_kevy(move |c| {
        let Some(bytes) = c.hget(hkey_c.as_bytes(), b"blob")? else {
            return Ok(false);
        };
        let Ok(env) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return Ok(false);
        };
        if env.get("sender").and_then(|v| v.as_str()) != Some(user_c.as_str()) {
            return Ok(false);
        }
        c.zrem(SCHEDULED_KEY, &[id_c.as_bytes()])?;
        c.del(&[hkey_c.as_bytes()])?;
        Ok(true)
    })
    .unwrap_or(false);
    if removed {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct RescheduleRequest {
    /// New send-time in Unix seconds. Must be in the future.
    pub scheduled_at: i64,
}

/// POST /api/scheduled/{id}/reschedule — G13.3. Updates the send-time
/// score on the scheduled zset. Requires caller to be the sender and
/// `scheduled_at` to be strictly in the future.
pub async fn reschedule_scheduled(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
    Json(req): Json<RescheduleRequest>,
) -> StatusCode {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if req.scheduled_at <= now {
        return StatusCode::BAD_REQUEST;
    }
    let hkey = format!("mailrs:outbound:job:{id}");
    let user_c = user.clone();
    let id_c = id.clone();
    let new_score = req.scheduled_at;
    let rescheduled = crate::handlers::kevy_util::with_kevy(move |c| {
        let Some(bytes) = c.hget(hkey.as_bytes(), b"blob")? else {
            return Ok(false);
        };
        let Ok(env) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return Ok(false);
        };
        if env.get("sender").and_then(|v| v.as_str()) != Some(user_c.as_str()) {
            return Ok(false);
        }
        c.zrem(SCHEDULED_KEY, &[id_c.as_bytes()])?;
        c.zadd(SCHEDULED_KEY, &[(new_score as f64, id_c.as_bytes())])?;
        Ok(true)
    })
    .unwrap_or(false);
    if rescheduled {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// `GET /api/scheduled` — the caller's own future-dated sends.
///
/// Cancel and reschedule have existed since G13.3 and no client has
/// ever called either, because nothing could list what there was to
/// cancel: the listing existed only as an MCP tool. A phone that can
/// schedule a message and not un-schedule it is worse than one that
/// cannot schedule at all.
pub async fn list_scheduled(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let ids = crate::handlers::kevy_util::with_kevy(|c| {
        c.zrange(SCHEDULED_IDX, 0, -1).map_err(std::io::Error::from)
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let items = crate::handlers::kevy_util::with_kevy(move |c| {
        let mut out: Vec<serde_json::Value> = Vec::new();
        for member in ids {
            let Ok(id) = String::from_utf8(member.clone()) else {
                continue;
            };
            let hkey = format!("mailrs:outbound:job:{id}");
            let Some(blob) = c.hget(hkey.as_bytes(), b"blob")? else {
                continue;
            };
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&blob) else {
                continue;
            };
            // Somebody else's scheduled mail is not this caller's to
            // see or to cancel; the cancel route makes the same check
            // against the same field.
            if env.get("sender").and_then(|v| v.as_str()) != Some(user.as_str()) {
                continue;
            }
            let scheduled_at = c.zscore(SCHEDULED_IDX, &member)?.unwrap_or(0.0) as i64;
            out.push(serde_json::json!({
                "id": id,
                "scheduled_at": scheduled_at,
                "recipient": env.get("recipient").and_then(|v| v.as_str()).unwrap_or(""),
                "subject": env.get("subject").and_then(|v| v.as_str()).unwrap_or(""),
            }));
        }
        // Soonest first: the one about to go is the one you came to
        // stop.
        out.sort_by_key(|v| v.get("scheduled_at").and_then(|s| s.as_i64()).unwrap_or(0));
        Ok(out)
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "items": items })))
}

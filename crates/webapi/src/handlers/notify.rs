//! A reader's push settings and how often pushes happened.
//!
//! - `GET|PUT /api/notify` — the overall mode and quiet hours
//! - `GET /api/notify/overrides` — the per-sender and per-conversation choices
//! - `PUT /api/notify/senders/{address}`, `PUT /api/notify/threads/{thread_id}`
//! - `GET /api/notify/stats` — the last days' counters
//! - `GET /api/contacts/important`, `PUT /api/contacts/important/{address}`

use axum::Json;
use axum::extract::{Extension, Path, Query};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::{contacts, notify};

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// GET /api/notify
pub async fn get_prefs(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<notify::Prefs>, StatusCode> {
    with_kevy(move |c| notify::prefs(c, &user)).map(Json)
}

/// PUT /api/notify
pub async fn put_prefs(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(p): Json<notify::Prefs>,
) -> Result<StatusCode, StatusCode> {
    let day = 24 * 60;
    let window_ok = match (p.quiet_start, p.quiet_end) {
        (None, None) => true,
        (Some(s), Some(e)) => s < day && e < day && s != e,
        _ => false,
    };
    let offset_ok = (-14 * 60..=14 * 60).contains(&p.utc_offset_minutes);
    if !window_ok || !offset_ok {
        return Err(StatusCode::BAD_REQUEST);
    }
    with_kevy(move |c| notify::put_prefs(c, &user, &p))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Serialize)]
pub struct Overrides {
    pub senders: Vec<(String, String)>,
    pub threads: Vec<(String, String)>,
}

/// GET /api/notify/overrides
pub async fn overrides(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Overrides>, StatusCode> {
    with_kevy(move |c| {
        Ok(Overrides {
            senders: notify::overrides(c, &notify::senders_key(&user))?,
            threads: notify::overrides(c, &notify::threads_key(&user))?,
        })
    })
    .map(Json)
}

#[derive(Debug, serde::Deserialize)]
pub struct OverrideBody {
    /// `null` removes the choice.
    pub value: Option<String>,
}

/// PUT /api/notify/senders/{address} — `always`, `never` or null.
pub async fn put_sender(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(body): Json<OverrideBody>,
) -> Result<StatusCode, StatusCode> {
    let address = mailrs_rfc5322::addr_spec(&address)
        .trim()
        .to_ascii_lowercase();
    let ok = body
        .value
        .as_deref()
        .is_none_or(|v| v == "always" || v == "never");
    if !ok || !address.contains('@') {
        return Err(StatusCode::BAD_REQUEST);
    }
    with_kevy(move |c| {
        notify::set_override(
            c,
            &notify::senders_key(&user),
            &address,
            body.value.as_deref(),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/notify/threads/{thread_id} — `follow`, `mute` or null.
pub async fn put_thread(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    Json(body): Json<OverrideBody>,
) -> Result<StatusCode, StatusCode> {
    let ok = body
        .value
        .as_deref()
        .is_none_or(|v| v == "follow" || v == "mute");
    if !ok || thread_id.is_empty() || thread_id.len() > 512 {
        return Err(StatusCode::BAD_REQUEST);
    }
    with_kevy(move |c| {
        notify::set_override(
            c,
            &notify::threads_key(&user),
            &thread_id,
            body.value.as_deref(),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
pub struct StatsQuery {
    #[serde(default = "seven")]
    pub days: u32,
}

fn seven() -> u32 {
    7
}

/// GET /api/notify/stats?days=7 — newest day first, at most 90.
pub async fn stats(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<Vec<notify::DayStats>>, StatusCode> {
    let days = q.days.clamp(1, 90);
    let now = chrono::Utc::now().timestamp();
    with_kevy(move |c| notify::stats(c, &user, days, now)).map(Json)
}

/// GET /api/contacts/important
pub async fn important(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<String>>, StatusCode> {
    with_kevy(move |c| contacts::vips(c, &user)).map(Json)
}

#[derive(Debug, serde::Deserialize)]
pub struct ImportantBody {
    pub important: bool,
}

/// PUT /api/contacts/important/{address}
pub async fn put_important(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(body): Json<ImportantBody>,
) -> Result<StatusCode, StatusCode> {
    let address = mailrs_rfc5322::addr_spec(&address)
        .trim()
        .to_ascii_lowercase();
    if !address.contains('@') {
        return Err(StatusCode::BAD_REQUEST);
    }
    with_kevy(move |c| contacts::set_vip(c, &user, &address, body.important))?;
    Ok(StatusCode::NO_CONTENT)
}

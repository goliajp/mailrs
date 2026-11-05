//! Drafts / signatures / templates — shared per-user side-state served
//! from the network kevy, keyed exactly like webapi + pg-core:
//!   `{family}:{user}`          hash id → JSON wire
//!   `{family}:{user}:counter`  string next id
//! Both cores read/write these same keys, so the route is backend-neutral.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use mailrs_core_api::method::admin::{
    DraftListResponse, DraftWire, SaveDraftRequest, SaveDraftResponse, SaveSignatureRequest,
    SaveSignatureResponse, SaveTemplateRequest, SaveTemplateResponse, SignatureWire,
    TemplateListResponse, TemplateWire,
};

use crate::NetKevy;

/// Best-effort epoch seconds. Fastcore forbids `Date::now`-style calls in
/// hot loops elsewhere, but a wall clock for a user-facing timestamp is
/// fine here.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// INCR the per-family counter and return the new id.
fn next_id(conn: &mut kevy_client::Connection, ckey: &str) -> Option<i64> {
    conn.incr(ckey.as_bytes()).ok()
}

/// HGETALL a `{family}:{user}` hash and deserialize the values (odd
/// entries) as `T`. Kevy returns a flat [field, value, field, value...].
fn hgetall_values<T: serde::de::DeserializeOwned>(
    conn: &mut kevy_client::Connection,
    key: &str,
) -> Vec<T> {
    let flat = match conn.hgetall(key.as_bytes()) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    flat.into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
        .filter_map(|v| serde_json::from_slice::<T>(&v).ok())
        .collect()
}

fn hset_json<T: serde::Serialize>(
    conn: &mut kevy_client::Connection,
    key: &str,
    id: i64,
    val: &T,
) -> Result<(), StatusCode> {
    let json = serde_json::to_vec(val).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.hset(
        key.as_bytes(),
        &[(id.to_string().as_bytes(), json.as_slice())],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

fn hdel_id(conn: &mut kevy_client::Connection, key: &str, id: i64) -> StatusCode {
    match conn.hdel(key.as_bytes(), &[id.to_string().as_bytes()]) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub fn drafts_key(user: &str) -> String {
    format!("drafts:{user}")
}

pub fn drafts_counter_key(user: &str) -> String {
    format!("drafts:{user}:counter")
}

/// The `withheld_send_at` draft `id` already carries, for a save that
/// rewrites it: the saving client does not send the field back, and a
/// draft a restore returned keeps saying when it was set to go.
pub fn kept_withheld_send_at(
    conn: &mut kevy_client::Connection,
    user: &str,
    id: i64,
) -> Option<i64> {
    let raw = conn
        .hget(drafts_key(user).as_bytes(), id.to_string().as_bytes())
        .ok()??;
    serde_json::from_slice::<DraftWire>(&raw)
        .ok()?
        .withheld_send_at
}

pub fn signatures_key(user: &str) -> String {
    format!("signatures:{user}")
}

pub fn signatures_counter_key(user: &str) -> String {
    format!("signatures:{user}:counter")
}

pub fn templates_key(user: &str) -> String {
    format!("templates:{user}")
}

pub fn templates_counter_key(user: &str) -> String {
    format!("templates:{user}:counter")
}

// ── drafts ──────────────────────────────────────────────────────────

pub async fn list_drafts<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
) -> Json<DraftListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(DraftListResponse { items: Vec::new() });
    };
    let mut items: Vec<DraftWire> = hgetall_values(&mut conn, &drafts_key(&user));
    items.sort_by_key(|d| -d.updated_at);
    Json(DraftListResponse { items })
}

pub async fn save_draft<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Json(req): Json<SaveDraftRequest>,
) -> Result<Json<SaveDraftResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    // upsert: reuse a client-supplied id (in-place update) or allocate a
    // fresh one — so autosave updates one draft instead of spawning many.
    let id = match req.id {
        Some(existing) => existing,
        None => next_id(&mut conn, &drafts_counter_key(&user))
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?,
    };
    let now = now_secs();
    let withheld_send_at = kept_withheld_send_at(&mut conn, &user, id);
    let draft = DraftWire {
        id,
        withheld_send_at,
        to: req.to,
        cc: req.cc,
        bcc: req.bcc,
        subject: req.subject,
        body: req.body,
        reply_to_thread_id: req.reply_to_thread_id,
        from: req.from,
        scheduled_at: req.scheduled_at,
        created_at: now,
        updated_at: now,
    };
    hset_json(&mut conn, &drafts_key(&user), id, &draft)?;
    Ok(Json(SaveDraftResponse { id }))
}

pub async fn delete_draft<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, id)): Path<(String, i64)>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    hdel_id(&mut conn, &drafts_key(&user), id)
}

// ── signatures ──────────────────────────────────────────────────────

pub async fn save_signature<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Json(req): Json<SaveSignatureRequest>,
) -> Result<Json<SaveSignatureResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let id = next_id(&mut conn, &signatures_counter_key(&user))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let sig = SignatureWire {
        id,
        name: req.name,
        html: req.html,
        text_content: req.text_content,
        is_default: req.is_default,
        created_at: now_secs().to_string(),
    };
    hset_json(&mut conn, &signatures_key(&user), id, &sig)?;
    Ok(Json(SaveSignatureResponse { id }))
}

pub async fn delete_signature<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, id)): Path<(String, i64)>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    hdel_id(&mut conn, &signatures_key(&user), id)
}

// ── templates ───────────────────────────────────────────────────────

pub async fn list_templates<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
) -> Json<TemplateListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(TemplateListResponse { items: Vec::new() });
    };
    let mut items: Vec<TemplateWire> = hgetall_values(&mut conn, &templates_key(&user));
    items.sort_by_key(|t| t.id);
    Json(TemplateListResponse { items })
}

pub async fn save_template<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Json(req): Json<SaveTemplateRequest>,
) -> Result<Json<SaveTemplateResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let id = next_id(&mut conn, &templates_counter_key(&user))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = now_secs().to_string();
    let tmpl = TemplateWire {
        id,
        name: req.name,
        subject: req.subject,
        html_body: req.html_body,
        text_body: req.text_body,
        category: req.category,
        is_default: req.is_default,
        created_at: now.clone(),
        updated_at: now,
    };
    hset_json(&mut conn, &templates_key(&user), id, &tmpl)?;
    Ok(Json(SaveTemplateResponse { id }))
}

pub async fn delete_template<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, id)): Path<(String, i64)>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    hdel_id(&mut conn, &templates_key(&user), id)
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn pref_keys_keep_their_prod_spelling() {
        assert_eq!(drafts_key("a@x.jp"), "drafts:a@x.jp");
        assert_eq!(drafts_counter_key("a@x.jp"), "drafts:a@x.jp:counter");
        assert_eq!(signatures_key("a@x.jp"), "signatures:a@x.jp");
        assert_eq!(
            signatures_counter_key("a@x.jp"),
            "signatures:a@x.jp:counter"
        );
        assert_eq!(templates_key("a@x.jp"), "templates:a@x.jp");
        assert_eq!(templates_counter_key("a@x.jp"), "templates:a@x.jp:counter");
    }
}

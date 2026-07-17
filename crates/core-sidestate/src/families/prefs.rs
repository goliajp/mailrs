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
    SaveSignatureResponse, SaveTemplateRequest, SaveTemplateResponse, SignatureListResponse,
    SignatureWire, TemplateListResponse, TemplateWire,
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

// ── drafts ──────────────────────────────────────────────────────────

pub async fn list_drafts<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
) -> Json<DraftListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(DraftListResponse { items: Vec::new() });
    };
    let mut items: Vec<DraftWire> = hgetall_values(&mut conn, &format!("drafts:{user}"));
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
        None => next_id(&mut conn, &format!("drafts:{user}:counter"))
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?,
    };
    let now = now_secs();
    let draft = DraftWire {
        id,
        to: req.to,
        cc: req.cc,
        bcc: req.bcc,
        subject: req.subject,
        body: req.body,
        reply_to_thread_id: req.reply_to_thread_id,
        created_at: now,
        updated_at: now,
    };
    hset_json(&mut conn, &format!("drafts:{user}"), id, &draft)?;
    Ok(Json(SaveDraftResponse { id }))
}

pub async fn delete_draft<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, id)): Path<(String, i64)>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    hdel_id(&mut conn, &format!("drafts:{user}"), id)
}

// ── signatures ──────────────────────────────────────────────────────

pub async fn list_signatures<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
) -> Json<SignatureListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(SignatureListResponse { items: Vec::new() });
    };
    let mut items: Vec<SignatureWire> = hgetall_values(&mut conn, &format!("signatures:{user}"));
    items.sort_by_key(|s| s.id);
    Json(SignatureListResponse { items })
}

pub async fn save_signature<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Json(req): Json<SaveSignatureRequest>,
) -> Result<Json<SaveSignatureResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let id = next_id(&mut conn, &format!("signatures:{user}:counter"))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let sig = SignatureWire {
        id,
        name: req.name,
        html: req.html,
        text_content: req.text_content,
        is_default: req.is_default,
        created_at: now_secs().to_string(),
    };
    hset_json(&mut conn, &format!("signatures:{user}"), id, &sig)?;
    Ok(Json(SaveSignatureResponse { id }))
}

pub async fn delete_signature<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, id)): Path<(String, i64)>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    hdel_id(&mut conn, &format!("signatures:{user}"), id)
}

// ── templates ───────────────────────────────────────────────────────

pub async fn list_templates<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
) -> Json<TemplateListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(TemplateListResponse { items: Vec::new() });
    };
    let mut items: Vec<TemplateWire> = hgetall_values(&mut conn, &format!("templates:{user}"));
    items.sort_by_key(|t| t.id);
    Json(TemplateListResponse { items })
}

pub async fn save_template<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Json(req): Json<SaveTemplateRequest>,
) -> Result<Json<SaveTemplateResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let id = next_id(&mut conn, &format!("templates:{user}:counter"))
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
    hset_json(&mut conn, &format!("templates:{user}"), id, &tmpl)?;
    Ok(Json(SaveTemplateResponse { id }))
}

pub async fn delete_template<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, id)): Path<(String, i64)>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    hdel_id(&mut conn, &format!("templates:{user}"), id)
}

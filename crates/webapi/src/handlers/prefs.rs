//! Fastcore-native handlers for user prefs — drafts, signatures,
//! templates, sender feedback.
//!
//! Storage lives in the shared network kevy so multiple webapi
//! instances can read/write. Keys:
//!
//! ```text
//!   drafts:<user>                        hash: draft_id -> JSON DraftWire
//!   drafts:<user>:counter                string: next id
//!   signatures:<user>                    hash: sig_id -> JSON SignatureWire
//!   signatures:<user>:counter            string: next id
//!   templates:<user>                     hash: tid -> JSON TemplateWire
//!   templates:<user>:counter             string: next id
//!   sender_feedback:<sender>             hash: action -> "1"
//! ```
//!
//! Zero spg touch. No fastcore RPC roundtrip (data lives in network
//! kevy which webapi already talks to for sessions).

use std::sync::Arc;

use crate::handlers::kevy_util::with_kevy;
use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

// The send path moved to `compose` and `send` on 2026-08-02. Re-exported
// so every `handlers::prefs::…` path in the router and the MCP tools keeps
// resolving — the split is about where the code lives, not about renaming
// forty call sites.
pub(crate) use crate::handlers::compose_attach::*;
pub(crate) use crate::handlers::prefs_misc::*;
pub(crate) use crate::handlers::send::*;
pub(crate) use crate::handlers::send_queue::*;

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Next id from a `<hash>:counter` string.
pub(crate) fn next_id(c: &mut kevy_client::Connection, counter_key: &str) -> std::io::Result<i64> {
    // v2 Stage B.2: single-op INCR — kevy-side atomic, no race.
    c.incr(counter_key.as_bytes()).map_err(std::io::Error::from)
}

// ── drafts ─────────────────────────────────────────────────────────

/// GET /api/mail/drafts — bare array of DraftWire.
pub async fn list_drafts(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<mailrs_core_api::method::admin::DraftWire>>, StatusCode> {
    let key = format!("drafts:{user}");
    let out = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))?;
    let mut drafts = Vec::new();
    for val in out
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
    {
        if let Ok(d) = serde_json::from_slice::<mailrs_core_api::method::admin::DraftWire>(&val) {
            drafts.push(d);
        }
    }
    drafts.sort_by_key(|d| -d.updated_at);
    Ok(Json(drafts))
}

/// POST /api/mail/drafts — { id: N }
pub async fn save_draft(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<mailrs_core_api::method::admin::SaveDraftRequest>,
) -> Result<Json<mailrs_core_api::method::admin::SaveDraftResponse>, StatusCode> {
    let now = now_secs();
    let key = format!("drafts:{user}");
    let ckey = format!("drafts:{user}:counter");
    let ckey_c = ckey.clone();
    // upsert: a client-supplied id reuses the same hash field (in-place
    // update); otherwise allocate a fresh id. hset overwrites either way.
    let id = match req.id {
        Some(existing) => existing,
        None => with_kevy(move |c| next_id(c, &ckey_c))?,
    };
    let draft = mailrs_core_api::method::admin::DraftWire {
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
    let json = serde_json::to_vec(&draft).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[(id.to_string().as_bytes(), json.as_slice())],
        )?;
        Ok(())
    })?;
    Ok(Json(mailrs_core_api::method::admin::SaveDraftResponse {
        id,
    }))
}

/// DELETE /api/mail/drafts/{id}
pub async fn delete_draft(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("drafts:{user}");
    with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[id.to_string().as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── signatures ────────────────────────────────────────────────────

/// GET /api/mail/signatures — bare array.
pub async fn list_signatures(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<mailrs_core_api::method::admin::SignatureWire>>, StatusCode> {
    let key = format!("signatures:{user}");
    let out = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))?;
    let mut items = Vec::new();
    for val in out
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
    {
        if let Ok(s) = serde_json::from_slice::<mailrs_core_api::method::admin::SignatureWire>(&val)
        {
            items.push(s);
        }
    }
    items.sort_by_key(|s| s.id);
    Ok(Json(items))
}

/// POST /api/mail/signatures
pub async fn save_signature(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<mailrs_core_api::method::admin::SaveSignatureRequest>,
) -> Result<Json<mailrs_core_api::method::admin::SaveSignatureResponse>, StatusCode> {
    let now = now_secs();
    let key = format!("signatures:{user}");
    let ckey = format!("signatures:{user}:counter");
    let ckey_c = ckey.clone();
    let id = with_kevy(move |c| next_id(c, &ckey_c))?;
    let sig = mailrs_core_api::method::admin::SignatureWire {
        id,
        name: req.name,
        html: req.html,
        text_content: req.text_content,
        is_default: req.is_default,
        created_at: now.to_string(),
    };
    let json = serde_json::to_vec(&sig).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[(id.to_string().as_bytes(), json.as_slice())],
        )?;
        Ok(())
    })?;
    Ok(Json(
        mailrs_core_api::method::admin::SaveSignatureResponse { id },
    ))
}

/// DELETE /api/mail/signatures/{id}
pub async fn delete_signature(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("signatures:{user}");
    with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[id.to_string().as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── templates ─────────────────────────────────────────────────────

/// GET /api/mail/templates
pub async fn list_templates(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<mailrs_core_api::method::admin::TemplateWire>>, StatusCode> {
    let key = format!("templates:{user}");
    let out = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))?;
    let mut items = Vec::new();
    for val in out
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
    {
        if let Ok(t) = serde_json::from_slice::<mailrs_core_api::method::admin::TemplateWire>(&val)
        {
            items.push(t);
        }
    }
    items.sort_by_key(|t| t.id);
    Ok(Json(items))
}

/// POST /api/mail/templates
pub async fn save_template(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<mailrs_core_api::method::admin::SaveTemplateRequest>,
) -> Result<Json<mailrs_core_api::method::admin::SaveTemplateResponse>, StatusCode> {
    let now = now_secs();
    let key = format!("templates:{user}");
    let ckey = format!("templates:{user}:counter");
    let ckey_c = ckey.clone();
    let id = with_kevy(move |c| next_id(c, &ckey_c))?;
    let t = mailrs_core_api::method::admin::TemplateWire {
        id,
        name: req.name,
        subject: req.subject,
        html_body: req.html_body,
        text_body: req.text_body,
        category: req.category,
        is_default: req.is_default,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    };
    let json = serde_json::to_vec(&t).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[(id.to_string().as_bytes(), json.as_slice())],
        )?;
        Ok(())
    })?;
    Ok(Json(mailrs_core_api::method::admin::SaveTemplateResponse {
        id,
    }))
}

/// DELETE /api/mail/templates/{id}
pub async fn delete_template(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("templates:{user}");
    with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[id.to_string().as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── sender feedback ────────────────────────────────────────────────

// ── BIMI ────────────────────────────────────────────────────────────

// ── proxy (image + link) ──────────────────────────────────────────

// ── /api/queue — outbound queue stats ─────────────────────────────

// ── /api/contacts — sender autocomplete ───────────────────────────

// ── /api/mail/send ────────────────────────────────────────────────

//! Fastcore-native handlers for every remaining route the React UI can
//! hit. Missing routes were making the dashboard / admin / password
//! reset flows either 404 or 500. Fill them all in — real
//! implementations where possible, safe empty defaults where the
//! feature isn't wired up yet.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

// Split by subject on 2026-08-02 — 1,687 lines holding password recovery,
// TOTP, system config, groups, audit and the agent surface. Re-exported so
// every `handlers::complete::…` path in the router keeps resolving.
pub(crate) use crate::handlers::apps_keys::*;
pub(crate) use crate::handlers::audit_read::*;
pub(crate) use crate::handlers::auth_recovery::*;
pub(crate) use crate::handlers::groups::*;
pub(crate) use crate::handlers::system_config::*;

pub(crate) fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let url = std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let handle = std::thread::spawn(move || -> std::io::Result<T> {
        let mut c = kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) fn hgetall_values(
    c: &mut kevy_client::Connection,
    key: &str,
) -> std::io::Result<Vec<Vec<u8>>> {
    let flat = c.hgetall(key.as_bytes()).map_err(std::io::Error::other)?;
    Ok(flat
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
        .collect())
}

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn next_id(c: &mut kevy_client::Connection, counter_key: &str) -> std::io::Result<i64> {
    // v2 Stage B.2: single-op INCR — kevy-side atomic, no read-modify-
    // write race. Prior get + parse + set could let two concurrent
    // /api/prefs writes both read the same current value and both
    // set the same next id, losing one row.
    c.incr(counter_key.as_bytes())
        .map_err(std::io::Error::other)
}

pub(crate) fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub(crate) fn map_core_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

// ── /api/mail/stats — dashboard summary ────────────────────────────

/// GET /api/mail/stats — combined dashboard counters from fastcore.
/// Runs three fastcore RPCs in parallel-like sequence and folds into
/// the shape `web/src/pages/dashboard.tsx` expects:
/// `{ categories, storage_bytes, total_messages, unread_messages }`.
pub async fn get_mail_stats(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let cats = state
        .core
        .conversation_categories(&user)
        .await
        .map_err(map_core_err)?;
    let unseen = state.core.unseen_count(&user).await.map_err(map_core_err)?;
    let total: i64 = cats.categories.iter().map(|c| c.count).sum();
    Ok(Json(serde_json::json!({
        "categories": cats.categories,
        "storage_bytes": 0,
        "total_messages": total,
        "unread_messages": unseen.count,
    })))
}

// ── Auth extras (OIDC / password reset / recovery / TOTP) ─────────

// ── /api/mail/keys/status — PGP setup status ──────────────────────

// ── /api/mail/messages/{uid} — single message (metadata + body) ───

pub async fn get_message_single(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(uid): Path<u32>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let w = state
        .core
        .get_message_by_uid_for_user(&user, uid)
        .await
        .map_err(map_core_err)?;
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let mut text_body: Option<String> = None;
    let mut html_body: Option<String> = None;
    if let Some((local, domain)) = user.split_once('@') {
        let path = format!("{maildir_root}/{domain}/{local}");
        use mailrs_message_store::MessageStore;
        let store = mailrs_message_store::MaildirStore;
        let id = mailrs_message_store::MessageId(w.blob_ref.clone());
        if let Ok(Some(bytes)) = store.fetch(&path, &id).await {
            let root = mailrs_mime::parse(&bytes);
            for part in root.walk() {
                let mt = part.content_type.mime_type();
                if text_body.is_none() && mt == "text/plain" {
                    text_body = part.body_text();
                } else if html_body.is_none() && mt == "text/html" {
                    html_body = part.body_text();
                }
                if text_body.is_some() && html_body.is_some() {
                    break;
                }
            }
        }
    }
    // NOTE: legacy `id` field dropped 2026-07-08 (was always 0 under
    // fastcore's kevy-only architecture). Callers must use `uid` as
    // the per-user unique identity. See ThreadMessageResponse for the
    // matching rationale.
    Ok(Json(serde_json::json!({
        "uid": w.uid,
        "sender": mailrs_rfc2047::decode(w.sender.as_bytes()).into_owned(),
        "recipients": mailrs_rfc2047::decode(w.recipients.as_bytes()).into_owned(),
        "subject": w.subject,
        "internal_date": w.internal_date,
        "message_id": w.message_id,
        "text_body": text_body,
        "html_body": html_body,
        "flags": w.flags,
    })))
}

// ── /api/queue/{id}/retry — outbound queue retry ──────────────────

pub async fn queue_retry(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    // Set state=pending on the v2 job hash and LPUSH pending-idx —
    // the same shape sender's own retry path uses. The pre-2.9.38 form
    // wrote the legacy `mailrs:outbound:pending`, which sender had
    // stopped consuming, so retries silently no-op'd.
    let now = now_secs();
    with_kevy(move |c| {
        mailrs_core_sidestate::families::outbound::requeue_pending(c, id, now).map(|_| ())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Calendar (CalDAV placeholder) ──────────────────────────────────

pub async fn calendar_feeds() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "items": [] }))
}

#[derive(Debug, serde::Deserialize)]
pub struct CalendarConflictsQuery {
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
}

pub async fn calendar_conflicts(
    Query(_q): Query<CalendarConflictsQuery>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "conflicts": [] }))
}

// ── Admin: apps ────────────────────────────────────────────────────

pub(crate) const APPS_KEY: &str = "admin:apps";
pub(crate) const APPS_CTR: &str = "admin:apps:counter";

// ── Admin: audit-log messages/raw + audit/accounts + audit/conversations ────

// ── Admin: config/smtp + system-config ─────────────────────────────

// ── Admin: groups + permissions + group members ────────────────────

pub(crate) const GROUPS_KEY: &str = "admin:groups";
pub(crate) const GROUPS_CTR: &str = "admin:groups:counter";

// ── Admin: email-groups (distribution lists) ──────────────────────

pub(crate) const EG_KEY: &str = "admin:email-groups";
pub(crate) const EG_CTR: &str = "admin:email-groups:counter";

// ── Admin: greylist local-lists ───────────────────────────────────

pub(crate) const GL_KEY: &str = "admin:greylist:local-lists";
pub(crate) const GL_CTR: &str = "admin:greylist:local-lists:counter";

pub async fn list_greylist_local() -> Result<Json<serde_json::Value>, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, GL_KEY))?;
    let items: Vec<serde_json::Value> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

/// A local greylist entry as the client sends it.
///
/// These four fields are the contract the admin UI was built against and
/// the one the monolith implements
/// (`crates/server/src/web/admin/greylist_local.rs`, backed by the
/// `greylist_local_lists` table). This handler was written with
/// `{address_or_domain, list_type}` — invented, overlapping neither — so
/// every create failed with a missing-field 422 and the read side returned
/// records the UI could not display either.
#[derive(Debug, serde::Deserialize)]
pub struct CreateGreylistRequest {
    /// `domain` or `address` — what `value` is.
    pub kind: String,
    /// `whitelist` or `blacklist`.
    pub list: String,
    pub value: String,
    /// Free-text reason. `null` and `""` both mean none; the UI sends
    /// `null` for an untouched field.
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn create_greylist_entry(
    Json(req): Json<CreateGreylistRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = with_kevy(|c| next_id(c, GL_CTR))?;
    let g = serde_json::json!({
        "id": id,
        "kind": req.kind,
        "list": req.list,
        "value": req.value,
        "note": req.note,
        "created_at": now_secs(),
    });
    let payload = serde_json::to_vec(&g).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            GL_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(Json(g))
}

pub async fn delete_greylist_entry(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(GL_KEY.as_bytes(), &[id.to_string().as_bytes()])
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Admin: outbound queue admin view ──────────────────────────────

pub async fn list_admin_queue() -> Result<Json<serde_json::Value>, StatusCode> {
    // Return the last 100 IDs in pending + inflight.
    // v2: read the pending-idx sender actually drains; the legacy
    // `mailrs:outbound:pending` / `mailrs:outbound:inflight` lists have
    // been dead since Phase 8.1 (see stone outbound.rs), so the old
    // read returned only stuck ghosts.
    let ids = with_kevy(|c| {
        let pending = c
            .lrange(b"mailrs:outbound:pending-idx", 0, 99)
            .unwrap_or_default();
        let inflight = c
            .lrange(b"mailrs:outbound:inflight", 0, 99)
            .unwrap_or_default();
        Ok((pending, inflight))
    })?;
    let mut items = Vec::new();
    for (label, list) in [("pending", &ids.0), ("inflight", &ids.1)] {
        for b in list {
            let id_str = String::from_utf8_lossy(b).to_string();
            let key = format!("mailrs:outbound:job:{id_str}");
            let key_c = key.clone();
            let blob = with_kevy(move |c| {
                c.hget(key_c.as_bytes(), b"blob")
                    .map_err(std::io::Error::other)
            })?;
            if let Some(b) = blob
                && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b)
            {
                let mut item = v;
                if let Some(o) = item.as_object_mut() {
                    o.insert("status".into(), serde_json::Value::String(label.into()));
                }
                items.push(item);
            }
        }
    }
    Ok(Json(serde_json::json!({ "items": items })))
}

// ── Agent: keys + webhooks (per-user) ─────────────────────────────

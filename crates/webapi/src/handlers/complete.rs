//! Fastcore-native handlers for every remaining route the React UI can
//! hit. Missing routes were making the dashboard / admin / password
//! reset flows either 404 or 500. Fill them all in — real
//! implementations where possible, safe empty defaults where the
//! feature isn't wired up yet.

use std::sync::Arc;

use crate::handlers::kevy_util::with_kevy;
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use mailrs_core_sidestate::families::outbound::PENDING_IDX;

// Split by subject on 2026-08-02 — 1,687 lines holding password recovery,
// TOTP, system config, groups, audit and the agent surface. Re-exported so
// every `handlers::complete::…` path in the router keeps resolving.
pub(crate) use crate::handlers::apps_keys::*;
pub(crate) use crate::handlers::audit_read::*;
pub(crate) use crate::handlers::auth_recovery::*;
pub(crate) use crate::handlers::groups::*;
pub(crate) use crate::handlers::system_config::*;

pub(crate) fn hgetall_values(
    c: &mut kevy_client::Connection,
    key: &str,
) -> std::io::Result<Vec<Vec<u8>>> {
    let flat = c.hgetall(key.as_bytes())?;
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
    c.incr(counter_key.as_bytes()).map_err(std::io::Error::from)
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
    // NOT the sum of `categories`. That sum is structurally an
    // undercount: the histogram omits junk on purpose, so adding it up
    // reports a mailbox smaller than the one `/api/mail/folders` shows
    // one panel lower on the same screen. Take the total from the same
    // place that panel does.
    let total: i64 = state
        .core
        .list_mailboxes(&user)
        .await
        .map_err(map_core_err)?
        .items
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("INBOX"))
        .map(|m| i64::from(m.uidnext.saturating_sub(1)))
        .unwrap_or(0);
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
    // The invitation, when the row says there is one. Asked of the
    // core rather than read here: the event lives in fastcore's
    // embedded kevy and this process talks to the *network* one — the
    // distinction that made `mark_not_junk` write into a store nobody
    // read.
    let invite_payload = if w.invite_method.is_empty() {
        None
    } else {
        state.core.get_invite(&user, w.uid).await.ok()
    };
    // The answer this reader already gave, if any. Written by
    // `handlers::invites` into the *network* store under
    // `rsvp:{user}:{uid}` — keyed by uid because that is what the route
    // takes and what the client sends.
    let (rsvp_status, rsvp_at) = if w.invite_method.is_empty() {
        (None, None)
    } else {
        let key = format!("rsvp:{user}:{}", w.uid);
        with_kevy(move |c| {
            let partstat = c
                .hget(key.as_bytes(), b"partstat")?
                .map(|v| String::from_utf8_lossy(&v).into_owned());
            let at = c
                .hget(key.as_bytes(), b"replied_at")?
                .map(|v| String::from_utf8_lossy(&v).into_owned());
            Ok((partstat, at))
        })
        .unwrap_or((None, None))
    };
    // Dates a person wrote in prose, for mail that carries no calendar
    // part — which is most mail about a meeting.
    //
    // `propose`, not `find`: reading a date and reading a proposal are
    // different questions, and asking only the first one put eight
    // chips on a support reply whose dates were all quoted SMTP
    // rejection timestamps. Computed when a
    // message is opened rather than at delivery: it is one pass over a
    // body that is already in hand here, and putting it on the ingest
    // path would spend it on every newsletter that will never be read.
    //
    // Proposals, not events. Nothing is filed until somebody says so.
    let date_suggestions: Vec<serde_json::Value> = if w.invite_method.is_empty() {
        let reference = chrono::DateTime::from_timestamp(w.internal_date, 0)
            .map(|d| d.date_naive())
            .unwrap_or_else(|| chrono::Utc::now().date_naive());
        let body = text_body.as_deref().unwrap_or_default();
        mailrs_datefind::propose(body, reference)
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "text": c.text,
                    "date": c.date.to_string(),
                    // Wall-clock, deliberately: the writer meant their
                    // own hour, and this side does not know which zone
                    // that is. The client renders it as local, which is
                    // the same guess the reader would make.
                    "datetime": c.naive().map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string()),
                })
            })
            .collect()
    } else {
        Vec::new()
    };
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
        // Empty string, not null, for the overwhelming majority that
        // carry no calendar part: the field is a method name or it is
        // absent, and the client tests it for truthiness either way.
        "invite_method": w.invite_method,
        "invite_payload": invite_payload,
        "rsvp_status": rsvp_status,
        "rsvp_at": rsvp_at,
        "date_suggestions": date_suggestions,
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
        )?;
        Ok(())
    })?;
    Ok(Json(g))
}

pub async fn delete_greylist_entry(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(GL_KEY.as_bytes(), &[id.to_string().as_bytes()])?;
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
        let pending = c.lrange(PENDING_IDX, 0, 99).unwrap_or_default();
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
                    .map_err(std::io::Error::from)
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

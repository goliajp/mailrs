//! The odds and ends that shared `prefs.rs` with drafts and signatures:
//! BIMI logos, the image and link proxies, queue stats, contacts, and the
//! sender-feedback submission.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::prefs::{now_secs, with_kevy};
use mailrs_core_sidestate::families::outbound::PENDING_IDX;

#[derive(Debug, serde::Deserialize)]
pub struct FeedbackRequest {
    /// The address the feedback is about.
    ///
    /// Named `sender_email`, not `sender`, because that is what the client
    /// sends and what the monolith's `FeedbackRequest` has always taken
    /// (`crates/server/src/web/conversations/mod.rs`). This handler was
    /// written with `sender`, so every feedback submission failed
    /// deserialization with a missing-field 422 and no sender reputation
    /// was ever recorded on the fastcore lane.
    pub sender_email: String,
    pub action: String,
}

/// POST /api/mail/feedback — record sender reputation feedback.
/// Kevy hash `sender_feedback:<sender>` field=action value=timestamp.
pub(crate) async fn submit_feedback(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
    Json(req): Json<FeedbackRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("sender_feedback:{}", req.sender_email);
    let action = req.action;
    let ts = now_secs().to_string();
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(action.as_bytes(), ts.as_bytes())])
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/bimi/{domain} — DNS TXT lookup for `default._bimi.{domain}`
/// and return the parsed SVG URL. Trivial handler; no kevy or spg.
/// Response: `{ "l": "https://...svg", "a": "https://...pem" }` or 404.
pub(crate) async fn get_bimi(
    Path(domain): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let record = format!("default._bimi.{domain}");
    // Fresh resolver per request — same pattern as monolith's DNS layer.
    let resolver = hickory_resolver::TokioResolver::builder_tokio()
        .and_then(|b| b.build())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let lookup = resolver
        .txt_lookup(&record)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mut logo: Option<String> = None;
    let mut cert: Option<String> = None;
    for record in lookup.answers() {
        let hickory_resolver::proto::rr::RData::TXT(txt) = &record.data else {
            continue;
        };
        let joined = txt.to_string();
        for kv in joined.split(';') {
            let kv = kv.trim();
            if let Some(v) = kv.strip_prefix("l=") {
                logo = Some(v.trim().to_string());
            }
            if let Some(v) = kv.strip_prefix("a=") {
                cert = Some(v.trim().to_string());
            }
        }
    }
    let out = serde_json::json!({
        "l": logo,
        "a": cert,
    });
    if logo.is_none() && cert.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(out))
}

/// GET /api/proxy/image?url= — fetch external image bytes, rewrite
/// content-type. Simple allowlist by scheme (https/http).
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ProxyQuery {
    pub url: String,
}

pub(crate) async fn proxy_image(
    axum::extract::Query(q): axum::extract::Query<ProxyQuery>,
) -> Result<axum::response::Response, StatusCode> {
    if !q.url.starts_with("https://") && !q.url.starts_with("http://") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let resp = reqwest::get(&q.url)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let bytes = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let mut r = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", ct)
        .body(axum::body::Body::from(bytes.to_vec()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    r.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("public, max-age=3600"),
    );
    Ok(r)
}

/// GET /api/proxy/link?url= — 302 redirect to the given URL. Same
/// scheme allowlist. Purely a redirect stub; no tracking.
pub(crate) async fn proxy_link(
    axum::extract::Query(q): axum::extract::Query<ProxyQuery>,
) -> Result<axum::response::Response, StatusCode> {
    if !q.url.starts_with("https://") && !q.url.starts_with("http://") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let r = axum::response::Response::builder()
        .status(StatusCode::FOUND)
        .header("location", &q.url)
        .body(axum::body::Body::empty())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(r)
}

/// GET /api/queue — placeholder stats reading from kevy outbound
/// pending list. Returns `{ pending, inflight, suppression }`.
pub(crate) async fn get_queue_stats(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let out = with_kevy(|c| {
        let pending = c.llen(PENDING_IDX).unwrap_or(0) as i64;
        let inflight = c.llen(b"mailrs:outbound:inflight").unwrap_or(0) as i64;
        let suppression = c.scard(b"mailrs:outbound:suppression").unwrap_or(0) as i64;
        Ok(serde_json::json!({
            "pending": pending,
            "inflight": inflight,
            "suppression": suppression,
        }))
    })?;
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ContactsQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_contacts_limit")]
    pub limit: u32,
}

pub(crate) fn default_contacts_limit() -> u32 {
    5
}

/// GET /api/contacts?q=&limit= — returns `Vec<String>` where each
/// entry is a `Name <email>` formatted contact. Backed by the
/// `mailrs:user:<u>:contacts` kevy hash (email -> `Name <email>`),
/// populated by `mailrs-fastcore-backfill-contacts` on first run
/// and kept in sync by future `record_message_arrival` writes.
pub(crate) async fn get_contacts(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    axum::extract::Query(q): axum::extract::Query<ContactsQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let key = format!("mailrs:user:{user}:contacts");
    let query = q.q.to_lowercase();
    let limit = q.limit.max(1) as usize;
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::other))?;
    // hgetall returns [field, value, field, value, ...] — extract pairs.
    let mut matches: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let email = String::from_utf8_lossy(&flat[i]).to_lowercase();
        let display = String::from_utf8_lossy(&flat[i + 1]).to_string();
        if email.contains(&query) || display.to_lowercase().contains(&query) {
            matches.push(display);
        }
        i += 2;
        if matches.len() >= limit * 4 {
            break;
        }
    }
    matches.sort();
    matches.dedup();
    matches.truncate(limit);
    Ok(Json(matches))
}

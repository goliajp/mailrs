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

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// Blocking helper — runs a closure with a fresh kevy connection.
/// Same pattern as `session::resolve_session`.
fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let url = std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let handle = std::thread::spawn(move || -> std::io::Result<T> {
        let mut c = kevy_client::Connection::open(&url)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Next id from a `<hash>:counter` string.
fn next_id(c: &mut kevy_client::Connection, counter_key: &str) -> std::io::Result<i64> {
    let cur = c.get(counter_key.as_bytes())?;
    let n = match cur {
        Some(bytes) => String::from_utf8_lossy(&bytes).parse::<i64>().unwrap_or(0) + 1,
        None => 1,
    };
    c.set(counter_key.as_bytes(), n.to_string().as_bytes())?;
    Ok(n)
}

// ── drafts ─────────────────────────────────────────────────────────

/// GET /api/mail/drafts — bare array of DraftWire.
pub async fn list_drafts(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<mailrs_core_api::method::admin::DraftWire>>, StatusCode> {
    let key = format!("drafts:{user}");
    let out = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
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
    let id = with_kevy(move |c| next_id(c, &ckey_c))?;
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
    let out = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
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
    let out = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
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

#[derive(Debug, serde::Deserialize)]
pub struct FeedbackRequest {
    pub sender: String,
    pub action: String,
}

/// POST /api/mail/feedback — record sender reputation feedback.
/// Kevy hash `sender_feedback:<sender>` field=action value=timestamp.
pub async fn submit_feedback(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
    Json(req): Json<FeedbackRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("sender_feedback:{}", req.sender);
    let action = req.action;
    let ts = now_secs().to_string();
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(action.as_bytes(), ts.as_bytes())])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── BIMI ────────────────────────────────────────────────────────────

/// GET /api/bimi/{domain} — DNS TXT lookup for `default._bimi.{domain}`
/// and return the parsed SVG URL. Trivial handler; no kevy or spg.
/// Response: `{ "l": "https://...svg", "a": "https://...pem" }` or 404.
pub async fn get_bimi(Path(domain): Path<String>) -> Result<Json<serde_json::Value>, StatusCode> {
    use hickory_resolver::config::{ResolverConfig, ResolverOpts};
    let record = format!("default._bimi.{domain}");
    // Fresh resolver per request — same pattern as monolith's DNS layer.
    let resolver = hickory_resolver::TokioAsyncResolver::tokio(
        ResolverConfig::default(),
        ResolverOpts::default(),
    );
    let lookup = resolver
        .txt_lookup(&record)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mut logo: Option<String> = None;
    let mut cert: Option<String> = None;
    for txt in lookup.iter() {
        let joined: String = txt
            .txt_data()
            .iter()
            .flat_map(|b| std::str::from_utf8(b).ok().map(str::to_owned))
            .collect::<Vec<_>>()
            .join("");
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

// ── proxy (image + link) ──────────────────────────────────────────

/// GET /api/proxy/image?url= — fetch external image bytes, rewrite
/// content-type. Simple allowlist by scheme (https/http).
#[derive(Debug, serde::Deserialize)]
pub struct ProxyQuery {
    pub url: String,
}

pub async fn proxy_image(
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
pub async fn proxy_link(
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

// ── /api/queue — outbound queue stats ─────────────────────────────

/// GET /api/queue — placeholder stats reading from kevy outbound
/// pending list. Returns `{ pending, inflight, suppression }`.
pub async fn get_queue_stats(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let out = with_kevy(|c| {
        let pending = c.llen(b"mailrs:outbound:pending").unwrap_or(0) as i64;
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

// ── /api/contacts — sender autocomplete ───────────────────────────

/// GET /api/contacts?q=&limit= — returns an empty list. Full
/// implementation reads from a `contacts:<user>` hash which is
/// currently populated by the pg-dump migration path but not yet
/// consulted here. Placeholder so the compose form doesn't 500.
pub async fn get_contacts(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(serde_json::json!({"items": []})))
}

// ── /api/mail/send ────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct SendRequest {
    pub to: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, serde::Serialize)]
pub struct SendResponse {
    pub queue_id: i64,
}

/// POST /api/mail/send — enqueue a plain RFC 5322 envelope in the
/// shared network kevy outbound queue for sender to pick up. Zero
/// spg touch. The sender binary consumes `mailrs:outbound:pending`
/// via LRANGE / LPOP.
pub async fn send_message(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SendRequest>,
) -> Result<Json<SendResponse>, StatusCode> {
    let ckey = "mailrs:outbound:counter".to_string();
    let ckey_c = ckey.clone();
    let id = with_kevy(move |c| next_id(c, &ckey_c))?;
    let message = format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\n\r\n{body}",
        from = user,
        to = req.to,
        subject = req.subject,
        body = req.body,
    );
    let envelope = serde_json::json!({
        "id": id,
        "sender": user,
        "recipient": req.to,
        "message_data": message,
        "created_at": now_secs(),
    });
    let payload = envelope.to_string();
    with_kevy(move |c| {
        c.hset(
            format!("mailrs:outbound:{id}").as_bytes(),
            &[(b"blob", payload.as_bytes())],
        )?;
        c.lpush(b"mailrs:outbound:pending", &[id.to_string().as_bytes()])?;
        Ok(())
    })?;
    Ok(Json(SendResponse { queue_id: id }))
}

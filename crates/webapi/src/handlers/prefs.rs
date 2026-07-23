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
    // v2 Stage B.2: single-op INCR — kevy-side atomic, no race.
    c.incr(counter_key.as_bytes())
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
        let pending = c.llen(b"mailrs:outbound:pending-idx").unwrap_or(0) as i64;
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

#[derive(Debug, serde::Deserialize)]
pub struct ContactsQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_contacts_limit")]
    pub limit: u32,
}

fn default_contacts_limit() -> u32 {
    5
}

/// GET /api/contacts?q=&limit= — returns `Vec<String>` where each
/// entry is a `Name <email>` formatted contact. Backed by the
/// `mailrs:user:<u>:contacts` kevy hash (email -> `Name <email>`),
/// populated by `mailrs-fastcore-backfill-contacts` on first run
/// and kept in sync by future `record_message_arrival` writes.
pub async fn get_contacts(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    axum::extract::Query(q): axum::extract::Query<ContactsQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let key = format!("mailrs:user:{user}:contacts");
    let query = q.q.to_lowercase();
    let limit = q.limit.max(1) as usize;
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
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

// ── /api/mail/send ────────────────────────────────────────────────

/// One entry parsed out of the compose form.
#[derive(Debug, Default)]
struct ComposeParts {
    from: String,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    body: String,
    html_body: String,
    in_reply_to: Option<String>,
    forward_message_id: Option<String>,
    attachments: Vec<Attachment>,
    /// Unix epoch seconds to send at; None / past = send now (G13).
    scheduled_at: Option<i64>,
}

#[derive(Debug)]
struct Attachment {
    filename: String,
    content_type: String,
    bytes: Vec<u8>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SendRequest {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub html_body: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub forward_message_id: Option<String>,
    #[serde(default)]
    pub scheduled_at: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct SendResponse {
    pub message_id: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// RFC 5322 date string in the shape smtpd wants.
fn rfc5322_date(epoch: i64) -> String {
    // Manual format so we don't pull in chrono here — the outbound
    // queue consumer re-parses this defensively anyway.
    // Sat, 02 Jul 2026 12:34:56 +0000
    static WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    static MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Simple date math — fine within the current epoch range.
    let secs = epoch.max(0) as u64;
    let mut days = secs / 86_400;
    let sec_of_day = secs % 86_400;
    let hour = (sec_of_day / 3600) as u32;
    let minute = ((sec_of_day % 3600) / 60) as u32;
    let second = (sec_of_day % 60) as u32;
    // 1970-01-01 was Thursday (index 4)
    let weekday = WEEKDAYS[((days + 4) % 7) as usize];
    let mut year: u32 = 1970;
    while {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let ydays = if leap { 366 } else { 365 };
        days >= ydays
    } {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        days -= if leap { 366 } else { 365 };
        year += 1;
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_lengths = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month: usize = 0;
    while month < 12 && days >= month_lengths[month] {
        days -= month_lengths[month];
        month += 1;
    }
    let day = days + 1;
    format!(
        "{weekday}, {day:02} {mon} {year} {hour:02}:{minute:02}:{second:02} +0000",
        mon = MONTHS[month],
    )
}

/// Encode a body buffer as base64 with 76-column line wrapping.
/// Universal-safe transport — no 8BITMIME dependence, no quoted-printable
/// pathological blow-up on CJK text.
fn base64_wrap(input: &[u8]) -> String {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(input);
    let mut out = String::with_capacity(encoded.len() + encoded.len() / 76 * 2);
    for chunk in encoded.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        out.push_str("\r\n");
    }
    out
}

/// Build a complete RFC 5322 envelope from parsed compose parts.
///
/// Transport-safe by construction:
/// - subject: RFC 2047 encoded-word
/// - attachment filenames: RFC 2231 (`filename*=UTF-8''<pct>`) so
///   non-ASCII filenames survive strict MTAs and older MUAs
/// - text/plain + text/html bodies: base64 (safe on non-8BITMIME hops,
///   avoids quoted-printable blow-up on CJK)
/// - attachment payload: base64 with 76-column line wrapping
fn build_rfc5322(parts: &ComposeParts, from: &str) -> (String, Vec<u8>) {
    let mid_local = random_hex(8);
    let mid_host = from.split('@').nth(1).unwrap_or("localhost");
    let message_id = format!("{mid_local}@{mid_host}");
    let date = rfc5322_date(now_secs());

    let has_attachments = !parts.attachments.is_empty();
    let has_html = !parts.html_body.is_empty();

    let mixed_boundary = format!("----=Mixed_{}", random_hex(6));
    let alt_boundary = format!("----=Alt_{}", random_hex(6));

    let mut out = String::new();
    out.push_str(&format!("Date: {date}\r\n"));
    out.push_str(&format!("From: {from}\r\n"));
    if !parts.to.is_empty() {
        out.push_str(&format!("To: {}\r\n", parts.to.join(", ")));
    }
    if !parts.cc.is_empty() {
        out.push_str(&format!("Cc: {}\r\n", parts.cc.join(", ")));
    }
    let encoded_subject = mailrs_rfc2047::encode(&parts.subject);
    out.push_str(&format!("Subject: {encoded_subject}\r\n"));
    out.push_str(&format!("Message-ID: <{message_id}>\r\n"));
    out.push_str("MIME-Version: 1.0\r\n");
    if let Some(ref irt) = parts.in_reply_to {
        out.push_str(&format!("In-Reply-To: <{irt}>\r\n"));
        out.push_str(&format!("References: <{irt}>\r\n"));
    }

    // Assemble text/alternative/mixed structure. We always emit an outer
    // Content-Type in the top-level header, then the body parts. When
    // there are attachments the outer is multipart/mixed; the first
    // inner part is either text/plain or multipart/alternative.
    let body_section = if has_html {
        let mut s = String::new();
        s.push_str(&format!(
            "Content-Type: multipart/alternative; boundary=\"{alt_boundary}\"\r\n\r\n"
        ));
        s.push_str(&format!("--{alt_boundary}\r\n"));
        s.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        s.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        s.push_str(&base64_wrap(parts.body.as_bytes()));
        s.push_str(&format!("\r\n--{alt_boundary}\r\n"));
        s.push_str("Content-Type: text/html; charset=utf-8\r\n");
        s.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        s.push_str(&base64_wrap(parts.html_body.as_bytes()));
        s.push_str(&format!("\r\n--{alt_boundary}--\r\n"));
        s
    } else {
        let mut s = String::new();
        s.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        s.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        s.push_str(&base64_wrap(parts.body.as_bytes()));
        s
    };

    if has_attachments {
        out.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{mixed_boundary}\"\r\n\r\n"
        ));
        out.push_str(&format!("--{mixed_boundary}\r\n"));
        out.push_str(&body_section);
    } else {
        out.push_str(&body_section);
    }

    let mut bytes = out.into_bytes();

    if has_attachments {
        for att in &parts.attachments {
            let mut part = String::new();
            let ct_name = mailrs_rfc2231::encode_param("name", &att.filename);
            let cd_name = mailrs_rfc2231::encode_param("filename", &att.filename);
            part.push_str(&format!("\r\n--{mixed_boundary}\r\n"));
            part.push_str(&format!(
                "Content-Type: {ct}; {ct_name}\r\n",
                ct = att.content_type,
            ));
            part.push_str(&format!("Content-Disposition: attachment; {cd_name}\r\n"));
            part.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
            part.push_str(&base64_wrap(&att.bytes));
            bytes.extend_from_slice(part.as_bytes());
        }
        bytes.extend_from_slice(format!("\r\n--{mixed_boundary}--\r\n").as_bytes());
    }

    (message_id, bytes)
}

/// Prepend the original message onto a forward's body/html and copy
/// its attachments across, so a compose whose isBackendForward path
/// sent only the user's leading line actually carries the forwarded
/// content the recipient expects to see.
///
/// The frontend contract (reply-box.tsx `isBackendForward`) is: when
/// `forward_message_id` is set, ship only the typed text — backend
/// inlines the original. `build_rfc5322` on its own ignores that
/// field, which is why every forward before 2.9.38 arrived as just
/// the leading line (no body, no attachments).
///
/// Best-effort: a lookup failure logs a warning and passes the send
/// through unmodified rather than fail — the compose already exists
/// on the caller's side and the operator can retry manually.
async fn inline_forward_content(state: &Arc<WebState>, user: &str, parts: &mut ComposeParts) {
    let Some(mid) = parts.forward_message_id.as_ref().filter(|s| !s.is_empty()) else {
        return;
    };
    let wire = match state.core.find_by_message_id_for_user(user, mid).await {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(error = %e, %user, message_id = %mid, "forward inline: message lookup failed");
            return;
        }
    };
    let raw = match super::messages::read_maildir_bytes_pub(user, &wire.blob_ref).await {
        Ok(r) => r,
        Err(status) => {
            tracing::warn!(
                status = ?status, %user, blob_ref = %wire.blob_ref,
                "forward inline: raw fetch failed"
            );
            return;
        }
    };
    let (orig_text, orig_html, _atts) = super::conversations::parse_body(&raw);

    // Attachments: walk the parsed MIME tree, skip the multipart
    // wrappers and the text/plain + text/html body parts (those went
    // into orig_text/orig_html above), and push everything else onto
    // parts.attachments. build_rfc5322 wraps the outer message in
    // multipart/mixed when parts.attachments is non-empty and lays
    // each attachment out with its filename + content-type + base64.
    //
    // A part is a body part when it's text/plain or text/html AND
    // does not have `Content-Disposition: attachment` (inline is a
    // body; attachment-disposition text is treated as a file). Same
    // rule the retired monolith send/text.rs used.
    let parsed = mailrs_mime::parse(&raw);
    for part in parsed.walk() {
        if part.content_type.is_multipart() {
            continue;
        }
        let mt = part.content_type.mime_type();
        let is_body_part = (mt == "text/plain" || mt == "text/html")
            && part
                .disposition
                .as_ref()
                .map(|d| !d.is_attachment())
                .unwrap_or(true);
        if is_body_part {
            continue;
        }
        let filename = part
            .attachment_filename()
            .map(String::from)
            .unwrap_or_else(|| "attachment".to_string());
        parts.attachments.push(Attachment {
            filename,
            content_type: mt,
            bytes: part.body.to_vec(),
        });
    }

    let user_text = std::mem::take(&mut parts.body);
    parts.body = match orig_text.as_deref() {
        Some(text) => format!("{user_text}\n\n---------- Forwarded message ----------\n{text}"),
        None => user_text.clone(),
    };

    // HTML: if the compose didn't submit HTML, synth a paragraph from
    // the plain text so a divider + forwarded HTML can be appended.
    let user_html_owned = std::mem::take(&mut parts.html_body);
    let user_html = if user_html_owned.is_empty() {
        format!("<p>{}</p>", user_text.replace('\n', "<br>"))
    } else {
        user_html_owned
    };
    parts.html_body = if let Some(html) = orig_html {
        format!(
            "{user_html}<hr style=\"border:none;border-top:1px solid #ccc;margin:16px 0\"><div style=\"color:#555\">{html}</div>"
        )
    } else if let Some(text) = orig_text.as_deref() {
        let escaped = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\n', "<br>");
        format!(
            "{user_html}<hr style=\"border:none;border-top:1px solid #ccc;margin:16px 0\"><pre style=\"font-family:sans-serif;white-space:pre-wrap\">{escaped}</pre>"
        )
    } else {
        user_html
    };
}

/// Enqueue outbound. When `scheduled_at` is a future epoch, the id
/// lands in the `mailrs:outbound:scheduled` zset (scored by send time)
/// instead of the pending list; the sender's due-sweep promotes it to
/// pending when the time arrives (G13). Past / None sends immediately.
fn enqueue_outbound_at(
    sender: &str,
    recipients: &[String],
    envelope: &[u8],
    scheduled_at: Option<i64>,
) -> Result<(), StatusCode> {
    let created = now_secs();
    let send_at = scheduled_at.filter(|t| *t > created);
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(envelope);
    let sender = sender.to_string();
    for rcpt in recipients {
        let rcpt = rcpt.trim().to_string();
        if rcpt.is_empty() {
            continue;
        }
        // Enqueue via the shared stone primitive so the write hits the
        // v2 job hash + pending-idx that sender actually reads. The
        // pre-2.9.38 form wrote a legacy `mailrs:outbound:{id}` +
        // `mailrs:outbound:pending`, which sender had stopped reading —
        // so every send from webapi silently stopped delivering until
        // an operator drained the queue by hand.
        let sender_c = sender.clone();
        let b64_c = b64.clone();
        let rcpt_c = rcpt.clone();
        with_kevy(move |c| {
            mailrs_core_sidestate::families::outbound::write_fresh_pending(
                c, &sender_c, &rcpt_c, &b64_c, send_at, None, created,
            )
            .map(|_| ())
        })?;
    }
    Ok(())
}

/// Take the first ~120 chars of `body` (or html-stripped `html_body`
/// if body is empty) as the preview shown in conversation lists.
fn build_preview(parts: &ComposeParts) -> String {
    let src = if !parts.body.is_empty() {
        parts.body.clone()
    } else if !parts.html_body.is_empty() {
        html2text::from_read(parts.html_body.as_bytes(), 80).unwrap_or_default()
    } else {
        String::new()
    };
    let cleaned = src.replace(['\r', '\n'], " ");
    if cleaned.chars().count() <= 120 {
        cleaned
    } else {
        cleaned.chars().take(120).collect::<String>() + "…"
    }
}

/// Mirror an outbound send / draft save into the sender's own kevy
/// view so it shows up in the Sent (or Drafts) tab immediately, into
/// their maildir so IMAP sees it, and into the contacts hash so
/// recipient autocomplete stays fresh.
///
/// `is_draft = true` marks the wire with `FLAG_DRAFT` and lands the
/// message under a Draft-flavored kevy thread; `false` marks
/// `FLAG_SEEN` (the sender always "already read" what they wrote).
///
/// This intentionally does one write per persistence layer and
/// swallows individual failures with a warning instead of failing the
/// whole request — the primary user-facing operation is the send
/// itself (kevy outbound queue), and the mirror is a UX nicety that
/// mustn't take the send down with it.
async fn mirror_send_to_sender_view(
    state: &Arc<WebState>,
    user: &str,
    parts: &ComposeParts,
    envelope: &[u8],
    message_id: &str,
    is_draft: bool,
) {
    use mailrs_core_api::method::message::MessageWire;
    use mailrs_core_api::method::thread::DeliverMessageRequest;
    use mailrs_message_store::{MaildirStore, MessageStore};

    let now = now_secs();
    // v2.9.5 threading fix — a reply must join the thread its parent
    // actually lives in (msgid → thread index via core-api), NOT be
    // keyed on the parent's raw Message-ID: for any thread deeper than
    // two messages that id differs from the inbound-path root, so the
    // sent copy fragmented into its own 1-message conversation.
    let mut thread_id: Option<String> = None;
    if let Some(irt) = &parts.in_reply_to
        && let Ok(resp) = state.core.find_thread_by_message_id(user, irt).await
    {
        thread_id = resp.thread_id;
    }
    let thread_id = match thread_id {
        Some(tid) => tid,
        None => parts
            .in_reply_to
            .clone()
            .unwrap_or_else(|| message_id.to_string()),
    };

    let (local, domain) = match user.split_once('@') {
        Some(v) => v,
        None => {
            tracing::warn!(%user, "mirror_send: malformed user address, skipping");
            return;
        }
    };
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let maildir_path = format!("{maildir_root}/{domain}/{local}");
    let store = MaildirStore;
    let blob_ref = match store.deliver_batch(&maildir_path, &[envelope]).await {
        Ok(ids) if !ids.is_empty() => {
            // sent copy counts against the sender's own quota
            let uk = format!("mailrs:quota:{}:used_bytes", user.to_lowercase());
            let n = envelope.len() as i64;
            let _ = crate::handlers::kevy_util::with_kevy(move |c| {
                c.incr_by(uk.as_bytes(), n)?;
                Ok(())
            });
            ids[0].0.clone()
        }
        Ok(_) => String::new(),
        Err(e) => {
            tracing::warn!(err = %e, %user, "mirror_send: maildir write failed, using synthetic blob_ref");
            format!("kevy:{message_id}")
        }
    };
    // Mark as read (sent) or draft in the maildir tag.
    if !blob_ref.is_empty() && !blob_ref.starts_with("kevy:") {
        let flag = if is_draft {
            mailrs_message_store::Flag::Draft
        } else {
            mailrs_message_store::Flag::Seen
        };
        let id = mailrs_message_store::MessageId(blob_ref.clone());
        if let Err(e) = store.mark_processed(&maildir_path, &id, &[flag]).await {
            tracing::debug!(err = %e, "mirror_send: mark_processed failed, continuing");
        }
    }

    let recipients_csv = parts.to.join(", ");
    let flags = if is_draft { 0b0001_0000 } else { 0b0000_0001 };
    let wire = MessageWire {
        id: 0,
        mailbox_id: 0,
        uid: 0,
        blob_ref: blob_ref.clone(),
        sender: user.to_string(),
        recipients: recipients_csv.clone(),
        subject: parts.subject.clone(),
        date: now,
        internal_date: now,
        size: envelope.len() as u32,
        flags,
        message_id: message_id.to_string(),
        in_reply_to: parts.in_reply_to.clone().unwrap_or_default(),
        // the user's own outbound copy — sender authentication is not a
        // meaningful question for mail we are sending ourselves.
        sender_trust: String::new(),
        thread_id: thread_id.clone(),
        modseq: 0,
        user_address: user.to_string(),
    };
    let wire_json = match serde_json::to_string(&wire) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(err = %e, "mirror_send: wire serialize failed");
            return;
        }
    };

    let req = DeliverMessageRequest {
        message_id: message_id.to_string(),
        subject: parts.subject.clone(),
        senders_csv: user.to_string(),
        latest_date: now,
        latest_preview: build_preview(parts),
        category: "inbox".to_string(),
        unread: false,
        uid: 0,
        payload_wire_json: wire_json,
    };
    if let Err(e) = state.core.deliver_message(user, &thread_id, &req).await {
        tracing::warn!(err = %e, %user, %thread_id, "mirror_send: fastcore deliver_message failed");
    }

    // Contacts autocomplete refresh — union of to+cc+bcc.
    let contact_targets: Vec<String> = parts
        .to
        .iter()
        .chain(parts.cc.iter())
        .chain(parts.bcc.iter())
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .collect();
    if !contact_targets.is_empty() {
        let user_owned = user.to_string();
        let now_ts = now_secs();
        let _ = with_kevy(move |c| {
            let key = format!("mailrs:user:{user_owned}:contacts");
            let ts_key = format!("mailrs:user:{user_owned}:contacts:ts");
            for raw in &contact_targets {
                let addr = extract_addr(raw);
                if addr.is_empty() {
                    continue;
                }
                let val = if raw.trim() != addr {
                    raw.trim().to_string()
                } else {
                    addr.clone()
                };
                c.hset(key.as_bytes(), &[(addr.as_bytes(), val.as_bytes())])?;
                // Track last-used ts in a companion zset so we can
                // evict the least-recently-emailed contacts once the
                // set grows past a soft cap. Without this the hash
                // grows unbounded.
                c.zadd(ts_key.as_bytes(), &[(now_ts as f64, addr.as_bytes())])?;
            }
            // Enforce a 2000-entry cap. If the zset exceeds it, drop
            // the oldest entries from both the hash and the zset.
            let size = c.zcard(ts_key.as_bytes())?;
            const CAP: usize = 2000;
            if size > CAP {
                let overflow = (size - CAP) as i64;
                let old = c.zrange(ts_key.as_bytes(), 0, overflow - 1)?;
                let old_refs: Vec<&[u8]> = old.iter().map(|v| v.as_slice()).collect();
                if !old_refs.is_empty() {
                    c.hdel(key.as_bytes(), &old_refs)?;
                    c.zrem(ts_key.as_bytes(), &old_refs)?;
                }
            }
            Ok(())
        });
    }
}

/// Extract the addr-spec (`user@host`) from an RFC 5322 mailbox token.
/// Mirrors sender.rs's helper — kept here so webapi doesn't depend on
/// the fastcore-sender bin crate.
fn extract_addr(raw: &str) -> String {
    let t = raw.trim();
    if let Some(start) = t.rfind('<')
        && let Some(end) = t.rfind('>')
        && end > start
    {
        return t[start + 1..end].trim().to_string();
    }
    t.to_string()
}

/// Return `Ok(())` iff `from` matches the authed user's own address
/// or any entry in their effective_permissions.send_as list.
/// Otherwise `Err(FORBIDDEN)` — this stops any authenticated user
/// from spoofing arbitrary From: (in particular, arbitrary domains).
async fn ensure_from_allowed(
    state: &Arc<WebState>,
    user: &str,
    from: &str,
) -> Result<(), StatusCode> {
    if from == user {
        return Ok(());
    }
    let perms = state
        .core
        .effective_permissions(user)
        .await
        .map_err(|_| StatusCode::FORBIDDEN)?;
    if perms.is_super || perms.send_as.iter().any(|s| s == from) {
        Ok(())
    } else {
        tracing::warn!(%user, %from, "send blocked: from not in send_as allowlist");
        Err(StatusCode::FORBIDDEN)
    }
}

/// POST /api/mail/send — JSON compose form, no attachments.
pub async fn send_message(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SendRequest>,
) -> Result<Json<SendResponse>, StatusCode> {
    let from = if req.from.is_empty() {
        user.clone()
    } else {
        req.from
    };
    ensure_from_allowed(&state, &user, &from).await?;
    let mut parts = ComposeParts {
        from: from.clone(),
        to: req.to,
        cc: req.cc,
        bcc: req.bcc,
        subject: req.subject,
        body: req.body,
        html_body: req.html_body,
        in_reply_to: req.in_reply_to,
        scheduled_at: req.scheduled_at,
        forward_message_id: req.forward_message_id,
        attachments: Vec::new(),
    };
    // If the compose is a forward, look up the original message and
    // prepend its body onto what the user typed. Pre-2.9.38 the field
    // was passed through untouched and build_rfc5322 ignored it, so
    // every forward sent only the user's leading text — the recipient
    // saw "FYI." with none of the forwarded content.
    inline_forward_content(&state, &user, &mut parts).await;
    let mut recipients = parts.to.clone();
    recipients.extend(parts.cc.clone());
    recipients.extend(parts.bcc.clone());
    let (message_id, envelope) = build_rfc5322(&parts, &from);
    enqueue_outbound_at(&user, &recipients, &envelope, parts.scheduled_at)?;
    mirror_send_to_sender_view(&state, &user, &parts, &envelope, &message_id, false).await;
    Ok(Json(SendResponse {
        message_id,
        success: true,
        message: None,
    }))
}

/// MCP-side send helper — same pipeline as [`send_message`] but without
/// the axum/JSON wrapper so the MCP tool can drive it directly. Returns
/// the assigned Message-ID on success.
#[allow(clippy::too_many_arguments)]
pub async fn send_email_mcp(
    state: &Arc<WebState>,
    auth_user: &str,
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    scheduled_at: Option<i64>,
) -> Result<String, String> {
    ensure_from_allowed(state, auth_user, from)
        .await
        .map_err(|c| format!("from not allowed ({c})"))?;
    let parts = ComposeParts {
        from: from.to_string(),
        to: to.to_vec(),
        cc: cc.to_vec(),
        bcc: Vec::new(),
        subject: subject.to_string(),
        body: body.to_string(),
        html_body: String::new(),
        in_reply_to: in_reply_to.map(|s| s.to_string()),
        forward_message_id: None,
        attachments: Vec::new(),
        scheduled_at,
    };
    let mut recipients = parts.to.clone();
    recipients.extend(parts.cc.clone());
    let (message_id, envelope) = build_rfc5322(&parts, from);
    // Same call the REST send handler uses — a future `scheduled_at`
    // lands the id in the scheduled zset instead of pending.
    enqueue_outbound_at(auth_user, &recipients, &envelope, parts.scheduled_at)
        .map_err(|c| format!("enqueue failed ({c})"))?;
    mirror_send_to_sender_view(state, auth_user, &parts, &envelope, &message_id, false).await;
    Ok(message_id)
}

/// POST /api/mail/send-multipart — multipart/form-data compose form.
/// Fields: from, to (repeated), cc (repeated), bcc (repeated), subject,
/// body, html_body, attachments (repeated file parts), in_reply_to,
/// scheduled_at, forward_message_id, forward_attachments_from.
pub async fn send_message_multipart(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<SendResponse>, StatusCode> {
    let mut parts = ComposeParts {
        from: user.clone(),
        ..Default::default()
    };
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "from" => parts.from = field.text().await.unwrap_or_default(),
            "to" => parts.to.push(field.text().await.unwrap_or_default()),
            "cc" => parts.cc.push(field.text().await.unwrap_or_default()),
            "bcc" => parts.bcc.push(field.text().await.unwrap_or_default()),
            "subject" => parts.subject = field.text().await.unwrap_or_default(),
            "body" => parts.body = field.text().await.unwrap_or_default(),
            "html_body" => parts.html_body = field.text().await.unwrap_or_default(),
            "in_reply_to" => parts.in_reply_to = Some(field.text().await.unwrap_or_default()),
            "forward_message_id" => {
                parts.forward_message_id = Some(field.text().await.unwrap_or_default())
            }
            "scheduled_at" => {
                parts.scheduled_at = field.text().await.ok().and_then(|s| s.trim().parse().ok())
            }
            "attachments" => {
                let filename = field.file_name().unwrap_or("attachment").to_string();
                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let bytes = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                parts.attachments.push(Attachment {
                    filename,
                    content_type,
                    bytes: bytes.to_vec(),
                });
            }
            _ => {
                let _ = field.text().await;
            }
        }
    }
    if parts.from.is_empty() {
        parts.from = user.clone();
    }
    ensure_from_allowed(&state, &user, &parts.from).await?;
    // Same fix as the JSON handler: prepend the forwarded original
    // when the request carries forward_message_id.
    inline_forward_content(&state, &user, &mut parts).await;
    let mut recipients = parts.to.clone();
    recipients.extend(parts.cc.clone());
    recipients.extend(parts.bcc.clone());
    let from = parts.from.clone();
    let (message_id, envelope) = build_rfc5322(&parts, &from);
    enqueue_outbound_at(&user, &recipients, &envelope, parts.scheduled_at)?;
    mirror_send_to_sender_view(&state, &user, &parts, &envelope, &message_id, false).await;
    Ok(Json(SendResponse {
        message_id,
        success: true,
        message: None,
    }))
}

#[cfg(test)]
mod build_rfc5322_tests {
    use super::*;

    fn parts(body: &str, atts: Vec<Attachment>) -> ComposeParts {
        ComposeParts {
            from: "a@example.com".into(),
            to: vec!["b@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "hello".into(),
            body: body.into(),
            html_body: String::new(),
            in_reply_to: None,
            forward_message_id: None,
            attachments: atts,
            scheduled_at: None,
        }
    }

    #[test]
    fn text_body_is_base64_not_8bit() {
        let (_mid, bytes) = build_rfc5322(&parts("hi 日本", vec![]), "a@example.com");
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.contains("Content-Transfer-Encoding: base64\r\n"));
        assert!(!s.contains("Content-Transfer-Encoding: 8bit"));
    }

    #[test]
    fn attachment_uses_rfc2231_for_japanese_filename() {
        let att = Attachment {
            filename: "取引明細.xlsx".into(),
            content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                .into(),
            bytes: b"hello".to_vec(),
        };
        let (_mid, bytes) = build_rfc5322(&parts("hi", vec![att]), "a@example.com");
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(
            s.contains("filename*=UTF-8''"),
            "expected RFC 2231 filename*=; body =\n{s}"
        );
        assert!(s.contains("name*=UTF-8''"), "expected RFC 2231 name*=");
        assert!(
            !s.contains("filename=\"取引明細"),
            "raw UTF-8 must not appear"
        );
    }

    #[test]
    fn attachment_ascii_filename_stays_legacy_quoted() {
        let att = Attachment {
            filename: "report.pdf".into(),
            content_type: "application/pdf".into(),
            bytes: b"x".to_vec(),
        };
        let (_mid, bytes) = build_rfc5322(&parts("hi", vec![att]), "a@example.com");
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.contains("filename=\"report.pdf\""));
    }

    #[test]
    fn multipart_mixed_wraps_alternative_when_html_and_attachments() {
        let att = Attachment {
            filename: "a.txt".into(),
            content_type: "text/plain".into(),
            bytes: b"x".to_vec(),
        };
        let mut p = parts("plain", vec![att]);
        p.html_body = "<p>html</p>".into();
        let (_mid, bytes) = build_rfc5322(&p, "a@example.com");
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.contains("multipart/mixed"));
        assert!(s.contains("multipart/alternative"));
    }
}

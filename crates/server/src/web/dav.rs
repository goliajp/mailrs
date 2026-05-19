//! CalDAV (RFC 4791) and CardDAV (RFC 6352) handlers.
//!
//! simplified implementation supporting Thunderbird, Apple Calendar/Contacts,
//! and other standard CalDAV/CardDAV clients.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use sha2::{Digest, Sha256};

use super::WebState;

// ── helpers ──────────────────────────────────────────────────────────

fn etag_of(data: &str) -> String {
    let hash = Sha256::digest(data.as_bytes());
    hex::encode(&hash[..8])
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn multistatus(inner: &str) -> Response {
    Response::builder()
        .status(207)
        .header("content-type", "application/xml; charset=utf-8")
        .header("dav", "1, 2, 3, calendar-access, addressbook")
        .body(axum::body::Body::from(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <D:multistatus xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\" \
             xmlns:CR=\"urn:ietf:params:xml:ns:carddav\" \
             xmlns:CS=\"http://calendarserver.org/ns/\">\n\
             {inner}\n\
             </D:multistatus>"
        )))
        .unwrap()
}

fn options_response() -> Response {
    Response::builder()
        .status(200)
        .header("dav", "1, 2, 3, calendar-access, addressbook")
        .header(
            "allow",
            "OPTIONS, GET, PUT, DELETE, PROPFIND, REPORT, MKCALENDAR",
        )
        .body(axum::body::Body::empty())
        .unwrap()
}

/// parse the Depth header from a request (defaults to 1 per RFC 4918)
fn parse_depth(headers: &HeaderMap) -> u32 {
    headers
        .get("depth")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| match s.trim() {
            "0" => Some(0),
            "1" => Some(1),
            "infinity" => Some(u32::MAX),
            _ => None,
        })
        .unwrap_or(1)
}

fn require_basic_auth() -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("www-authenticate", "Basic realm=\"mailrs\"")
        .body(axum::body::Body::from("authentication required"))
        .unwrap()
}

/// extract account address from HTTP Basic auth, verifying credentials
async fn authenticate(headers: &HeaderMap, state: &Arc<WebState>) -> Option<String> {
    // try Bearer first (for API key / session token compat)
    let auth_header = headers.get("authorization")?.to_str().ok()?;

    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        // reuse existing session lookup
        if let Some(session) = state.sessions.get(token)
            && session.created_at.elapsed() < super::SESSION_TTL {
                return Some(session.address.clone());
            }
        return None;
    }

    // HTTP Basic auth
    let encoded = auth_header.strip_prefix("Basic ")?;
    let decoded = String::from_utf8(
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).ok()?,
    )
    .ok()?;
    let (username, password) = decoded.split_once(':')?;

    let ds = state.domain_store.as_ref()?;
    let (account, password_hash) = match ds.get_account_with_hash(username).await.ok()? {
        Some(pair) => pair,
        None => {
            // constant-time: do dummy argon2 work even when account not found
            crate::users::dummy_verify(password);
            return None;
        }
    };
    if !account.active {
        return None;
    }

    let valid = if password_hash.starts_with("$argon2") {
        crate::users::UserStore::verify_hash(password, &password_hash)
    } else {
        password_hash == password
    };

    if valid {
        Some(account.address)
    } else {
        // try LDAP fallback
        if let Some(ref ldap) = state.ldap_config
            && ldap.authenticate(username, password).await {
                return Some(account.address);
            }
        None
    }
}

// ── ensure defaults ──────────────────────────────────────────────────

async fn ensure_default_calendar(pool: &sqlx::PgPool, address: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO calendars (account_address, name) VALUES ($1, 'Default') ON CONFLICT DO NOTHING",
    )
    .bind(address)
    .execute(pool)
    .await?;
    Ok(())
}

async fn ensure_default_addressbook(
    pool: &sqlx::PgPool,
    address: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO address_books (account_address, name) VALUES ($1, 'Default') ON CONFLICT DO NOTHING",
    )
    .bind(address)
    .execute(pool)
    .await?;
    Ok(())
}

// ── well-known redirects ─────────────────────────────────────────────

pub(super) async fn well_known_caldav() -> Redirect {
    Redirect::permanent("/dav/")
}

pub(super) async fn well_known_carddav() -> Redirect {
    Redirect::permanent("/dav/")
}

// ── principal (/dav/) ────────────────────────────────────────────────

pub(super) async fn dav_principal(
    method: Method,
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if method == Method::OPTIONS {
        return options_response();
    }

    let method_str = method.as_str();
    if method_str != "PROPFIND" {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };

    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let _ = ensure_default_calendar(pool, &address).await;
    let _ = ensure_default_addressbook(pool, &address).await;

    let user = &address;
    let cal_home = format!("/dav/calendars/{user}/");
    let card_home = format!("/dav/contacts/{user}/");

    // check what properties are requested
    let wants_current_user_principal = body.contains("current-user-principal");
    let wants_resourcetype = body.contains("resourcetype");
    let wants_displayname = body.contains("displayname");

    let mut props = String::new();
    if wants_current_user_principal || body.is_empty() {
        props.push_str(
            "<D:current-user-principal><D:href>/dav/</D:href></D:current-user-principal>\n"
        );
    }
    if wants_resourcetype || body.is_empty() {
        props.push_str("<D:resourcetype><D:collection/></D:resourcetype>\n");
    }
    if wants_displayname || body.is_empty() {
        props.push_str(&format!(
            "<D:displayname>{}</D:displayname>\n",
            xml_escape(user)
        ));
    }
    // always include home sets for discovery
    props.push_str(&format!(
        "<C:calendar-home-set><D:href>{cal_home}</D:href></C:calendar-home-set>\n\
         <CR:addressbook-home-set><D:href>{card_home}</D:href></CR:addressbook-home-set>\n"
    ));
    // principal-URL (some clients want this)
    props.push_str("<D:principal-URL><D:href>/dav/</D:href></D:principal-URL>\n");
    // supported report set
    props.push_str(
        "<D:supported-report-set>\
         <D:supported-report><D:report><C:calendar-multiget/></D:report></D:supported-report>\
         <D:supported-report><D:report><C:calendar-query/></D:report></D:supported-report>\
         <D:supported-report><D:report><CR:addressbook-multiget/></D:report></D:supported-report>\
         <D:supported-report><D:report><CR:addressbook-query/></D:report></D:supported-report>\
         </D:supported-report-set>\n",
    );

    multistatus(&format!(
        "<D:response>\n\
         <D:href>/dav/</D:href>\n\
         <D:propstat>\n<D:prop>\n{props}</D:prop>\n\
         <D:status>HTTP/1.1 200 OK</D:status>\n\
         </D:propstat>\n\
         </D:response>"
    ))
}

// ── calendar home (/dav/calendars/{user}/) ───────────────────────────

pub(super) async fn dav_calendar_home(
    method: Method,
    Path(user): Path<String>,
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    _body: String,
) -> Response {
    if method == Method::OPTIONS {
        return options_response();
    }
    if method.as_str() != "PROPFIND" {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }

    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let _ = ensure_default_calendar(pool, &address).await;

    let depth = parse_depth(&headers);

    let mut responses = format!(
        "<D:response>\n\
         <D:href>/dav/calendars/{user}/</D:href>\n\
         <D:propstat>\n<D:prop>\n\
         <D:resourcetype><D:collection/></D:resourcetype>\n\
         <D:displayname>Calendars</D:displayname>\n\
         </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
         </D:response>\n"
    );

    // only include child calendars at Depth: 1 or higher
    if depth >= 1 {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT name, color, description FROM calendars WHERE account_address = $1 ORDER BY name",
        )
        .bind(&address)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (name, color, description) in &rows {
            let encoded_name = urlencoding::encode(name);
            let href = format!("/dav/calendars/{user}/{encoded_name}/");
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{href}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:resourcetype><D:collection/><C:calendar/></D:resourcetype>\n\
                 <D:displayname>{}</D:displayname>\n\
                 <C:supported-calendar-component-set>\
                 <C:comp name=\"VEVENT\"/>\
                 </C:supported-calendar-component-set>\n\
                 <D:current-user-privilege-set>\
                 <D:privilege><D:all/></D:privilege>\
                 </D:current-user-privilege-set>\n\
                 <CS:getctag>{}</CS:getctag>\n\
                 <apple:calendar-color xmlns:apple=\"http://apple.com/ns/ical/\">{}</apple:calendar-color>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(name),
                xml_escape(description),
                xml_escape(color),
            ));
        }
    }

    multistatus(&responses)
}

// ── calendar collection (/dav/calendars/{user}/{calendar}/) ──────────

pub(super) async fn dav_calendar_collection(
    method: Method,
    Path((user, calendar)): Path<(String, String)>,
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if method == Method::OPTIONS {
        return options_response();
    }

    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }

    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let calendar_name = urlencoding::decode(&calendar)
        .unwrap_or_default()
        .into_owned();

    let cal_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM calendars WHERE account_address = $1 AND name = $2",
    )
    .bind(&address)
    .bind(&calendar_name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some(cal_id) = cal_id else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let depth = parse_depth(&headers);
    let method_str = method.as_str();
    match method_str {
        "PROPFIND" => {
            calendar_propfind(pool, &user, &calendar, cal_id, &body, depth).await
        }
        "REPORT" => {
            calendar_report(pool, &user, &calendar, cal_id, &body).await
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn calendar_propfind(
    pool: &sqlx::PgPool,
    user: &str,
    calendar: &str,
    cal_id: i64,
    _body: &str,
    depth: u32,
) -> Response {
    let href = format!("/dav/calendars/{user}/{calendar}/");
    let mut responses = format!(
        "<D:response>\n\
         <D:href>{href}</D:href>\n\
         <D:propstat>\n<D:prop>\n\
         <D:resourcetype><D:collection/><C:calendar/></D:resourcetype>\n\
         <D:displayname>{calendar}</D:displayname>\n\
         <C:supported-calendar-component-set><C:comp name=\"VEVENT\"/></C:supported-calendar-component-set>\n\
         </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
         </D:response>\n"
    );

    // only include child events at Depth: 1 or higher
    if depth >= 1 {
        let events = sqlx::query_as::<_, (String, String)>(
            "SELECT uid, etag FROM calendar_events WHERE calendar_id = $1",
        )
        .bind(cal_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (uid, etag) in &events {
            let event_href = format!("/dav/calendars/{user}/{calendar}/{uid}.ics");
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:getetag>\"{etag}\"</D:getetag>\n\
                 <D:getcontenttype>text/calendar; charset=utf-8</D:getcontenttype>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&event_href),
            ));
        }
    }

    multistatus(&responses)
}

async fn calendar_report(
    pool: &sqlx::PgPool,
    user: &str,
    calendar: &str,
    cal_id: i64,
    body: &str,
) -> Response {
    // determine report type
    let is_multiget = body.contains("calendar-multiget");

    if is_multiget {
        // extract hrefs from body
        let events = extract_multiget_events(pool, cal_id, body).await;
        let mut responses = String::new();
        for (uid, etag, icalendar) in &events {
            let event_href = format!("/dav/calendars/{user}/{calendar}/{uid}.ics");
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:getetag>\"{etag}\"</D:getetag>\n\
                 <C:calendar-data>{}</C:calendar-data>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&event_href),
                xml_escape(icalendar),
            ));
        }
        multistatus(&responses)
    } else {
        // calendar-query: return all events with calendar-data
        let events = sqlx::query_as::<_, (String, String, String)>(
            "SELECT uid, etag, icalendar FROM calendar_events WHERE calendar_id = $1",
        )
        .bind(cal_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut responses = String::new();
        for (uid, etag, icalendar) in &events {
            let event_href = format!("/dav/calendars/{user}/{calendar}/{uid}.ics");
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:getetag>\"{etag}\"</D:getetag>\n\
                 <C:calendar-data>{}</C:calendar-data>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&event_href),
                xml_escape(icalendar),
            ));
        }
        multistatus(&responses)
    }
}

/// extract UIDs from multiget href elements and fetch matching events
async fn extract_multiget_events(
    pool: &sqlx::PgPool,
    cal_id: i64,
    body: &str,
) -> Vec<(String, String, String)> {
    let mut uids = Vec::new();
    for href_start in body.match_indices("<D:href>").map(|(i, _)| i) {
        let rest = &body[href_start + 8..];
        if let Some(end) = rest.find("</D:href>") {
            let href = rest[..end].trim();
            // extract UID from href like /dav/calendars/user/cal/UID.ics
            if let Some(uid) = href.strip_suffix(".ics").and_then(|s| s.rsplit('/').next()) {
                uids.push(uid.to_string());
            }
        }
    }
    // also try <href> without namespace prefix
    for href_start in body.match_indices("<href>").map(|(i, _)| i) {
        let rest = &body[href_start + 6..];
        if let Some(end) = rest.find("</href>") {
            let href = rest[..end].trim();
            if let Some(uid) = href.strip_suffix(".ics").and_then(|s| s.rsplit('/').next()) {
                uids.push(uid.to_string());
            }
        }
    }

    if uids.is_empty() {
        return Vec::new();
    }

    // fetch matching events
    let placeholders: Vec<String> = uids.iter().enumerate().map(|(i, _)| format!("${}", i + 2)).collect();
    let query = format!(
        "SELECT uid, etag, icalendar FROM calendar_events WHERE calendar_id = $1 AND uid IN ({})",
        placeholders.join(", ")
    );

    let mut q = sqlx::query_as::<_, (String, String, String)>(&query).bind(cal_id);
    for uid in &uids {
        q = q.bind(uid);
    }
    q.fetch_all(pool).await.unwrap_or_default()
}

// ── event resource (/dav/calendars/{user}/{calendar}/{uid}) ──────────

pub(super) async fn dav_event(
    method: Method,
    Path((user, calendar, uid_file)): Path<(String, String, String)>,
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if method == Method::OPTIONS {
        return options_response();
    }

    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }

    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let calendar_name = urlencoding::decode(&calendar)
        .unwrap_or_default()
        .into_owned();
    let uid = uid_file.strip_suffix(".ics").unwrap_or(&uid_file);

    let cal_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM calendars WHERE account_address = $1 AND name = $2",
    )
    .bind(&address)
    .bind(&calendar_name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some(cal_id) = cal_id else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match method {
        Method::GET => event_get(pool, uid, cal_id).await,
        Method::PUT => event_put(pool, uid, cal_id, &headers, &body).await,
        Method::DELETE => event_delete(pool, uid, cal_id).await,
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn event_get(pool: &sqlx::PgPool, uid: &str, cal_id: i64) -> Response {
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT etag, icalendar FROM calendar_events WHERE calendar_id = $1 AND uid = $2",
    )
    .bind(cal_id)
    .bind(uid)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    match row {
        Some((etag, icalendar)) => Response::builder()
            .status(200)
            .header("content-type", "text/calendar; charset=utf-8")
            .header("etag", format!("\"{etag}\""))
            .body(axum::body::Body::from(icalendar))
            .unwrap(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn event_put(
    pool: &sqlx::PgPool,
    uid: &str,
    cal_id: i64,
    headers: &HeaderMap,
    body: &str,
) -> Response {
    // if-match check for conflict detection
    if let Some(if_match) = headers.get("if-match").and_then(|v| v.to_str().ok()) {
        let expected = if_match.trim_matches('"');
        let current: Option<String> = sqlx::query_scalar(
            "SELECT etag FROM calendar_events WHERE calendar_id = $1 AND uid = $2",
        )
        .bind(cal_id)
        .bind(uid)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        match current {
            Some(ref current_etag) if current_etag != expected => {
                return StatusCode::PRECONDITION_FAILED.into_response();
            }
            None => {
                return StatusCode::PRECONDITION_FAILED.into_response();
            }
            _ => {}
        }
    }

    // if-none-match: * means create-only (fail if exists)
    if let Some(if_none_match) = headers.get("if-none-match").and_then(|v| v.to_str().ok())
        && if_none_match.trim() == "*" {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM calendar_events WHERE calendar_id = $1 AND uid = $2)",
            )
            .bind(cal_id)
            .bind(uid)
            .fetch_one(pool)
            .await
            .unwrap_or(false);
            if exists {
                return StatusCode::PRECONDITION_FAILED.into_response();
            }
        }

    let etag = etag_of(body);

    // Prefer the structured iTIP-aware path (MRS-3): parse the iCalendar
    // body via `crate::ical`, project all RFC 5545 / 5546 fields into
    // calendar_events columns. Falls back to the legacy minimal path for
    // non-VEVENT objects (VTODO / VJOURNAL / VFREEBUSY) or parser failure
    // — those still write raw + summary so the CalDAV GET round-trip works.
    let parsed = if body.contains("BEGIN:VEVENT") {
        crate::ical::parse_invite(body.as_bytes()).ok()
    } else {
        None
    };

    let result = if let Some(ref parsed) = parsed {
        crate::calendar::upsert_from_parsed_invite(pool, cal_id, uid, parsed, body, &etag)
            .await
            .map(|_| ())
    } else {
        // Legacy minimal path (no VEVENT or parse failure).
        let summary = extract_ical_field(body, "SUMMARY");
        let dtstart = extract_ical_datetime(body, "DTSTART");
        let dtend = extract_ical_datetime(body, "DTEND");
        sqlx::query(
            "INSERT INTO calendar_events (calendar_id, uid, etag, icalendar, summary, dtstart, dtend)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (calendar_id, uid)
             WHERE recurrence_id IS NULL
             DO UPDATE SET etag = $3, icalendar = $4, summary = $5, dtstart = $6, dtend = $7, updated_at = now()",
        )
        .bind(cal_id)
        .bind(uid)
        .bind(&etag)
        .bind(body)
        .bind(&summary)
        .bind(dtstart)
        .bind(dtend)
        .execute(pool)
        .await
        .map(|_| ())
    };

    match result {
        Ok(()) => Response::builder()
            .status(StatusCode::CREATED)
            .header("etag", format!("\"{etag}\""))
            .body(axum::body::Body::empty())
            .unwrap(),
        Err(e) => {
            tracing::error!("dav event put error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn event_delete(pool: &sqlx::PgPool, uid: &str, cal_id: i64) -> Response {
    let result = sqlx::query(
        "DELETE FROM calendar_events WHERE calendar_id = $1 AND uid = $2",
    )
    .bind(cal_id)
    .bind(uid)
    .execute(pool)
    .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("dav event delete error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ── contact home (/dav/contacts/{user}/) ─────────────────────────────

pub(super) async fn dav_contact_home(
    method: Method,
    Path(user): Path<String>,
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    _body: String,
) -> Response {
    if method == Method::OPTIONS {
        return options_response();
    }
    if method.as_str() != "PROPFIND" {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }

    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let _ = ensure_default_addressbook(pool, &address).await;

    let depth = parse_depth(&headers);

    let mut responses = format!(
        "<D:response>\n\
         <D:href>/dav/contacts/{user}/</D:href>\n\
         <D:propstat>\n<D:prop>\n\
         <D:resourcetype><D:collection/></D:resourcetype>\n\
         <D:displayname>Address Books</D:displayname>\n\
         </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
         </D:response>\n"
    );

    // only include child address books at Depth: 1 or higher
    if depth >= 1 {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT name, description FROM address_books WHERE account_address = $1 ORDER BY name",
        )
        .bind(&address)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (name, description) in &rows {
            let encoded_name = urlencoding::encode(name);
            let href = format!("/dav/contacts/{user}/{encoded_name}/");
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{href}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:resourcetype><D:collection/><CR:addressbook/></D:resourcetype>\n\
                 <D:displayname>{}</D:displayname>\n\
                 <D:current-user-privilege-set>\
                 <D:privilege><D:all/></D:privilege>\
                 </D:current-user-privilege-set>\n\
                 <CS:getctag>{}</CS:getctag>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(name),
                xml_escape(description),
            ));
        }
    }

    multistatus(&responses)
}

// ── addressbook collection (/dav/contacts/{user}/{book}/) ────────────

pub(super) async fn dav_contact_collection(
    method: Method,
    Path((user, book)): Path<(String, String)>,
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if method == Method::OPTIONS {
        return options_response();
    }

    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }

    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let book_name = urlencoding::decode(&book).unwrap_or_default().into_owned();

    let book_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM address_books WHERE account_address = $1 AND name = $2",
    )
    .bind(&address)
    .bind(&book_name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some(book_id) = book_id else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let depth = parse_depth(&headers);
    match method.as_str() {
        "PROPFIND" => addressbook_propfind(pool, &user, &book, book_id, depth).await,
        "REPORT" => addressbook_report(pool, &user, &book, book_id, &body).await,
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn addressbook_propfind(
    pool: &sqlx::PgPool,
    user: &str,
    book: &str,
    book_id: i64,
    depth: u32,
) -> Response {
    let href = format!("/dav/contacts/{user}/{book}/");
    let mut responses = format!(
        "<D:response>\n\
         <D:href>{href}</D:href>\n\
         <D:propstat>\n<D:prop>\n\
         <D:resourcetype><D:collection/><CR:addressbook/></D:resourcetype>\n\
         <D:displayname>{book}</D:displayname>\n\
         </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
         </D:response>\n"
    );

    // only include child contacts at Depth: 1 or higher
    if depth >= 1 {
        let contacts = sqlx::query_as::<_, (String, String)>(
            "SELECT uid, etag FROM contacts WHERE address_book_id = $1",
        )
        .bind(book_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (uid, etag) in &contacts {
            let contact_href = format!("/dav/contacts/{user}/{book}/{uid}.vcf");
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:getetag>\"{etag}\"</D:getetag>\n\
                 <D:getcontenttype>text/vcard; charset=utf-8</D:getcontenttype>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&contact_href),
            ));
        }
    }

    multistatus(&responses)
}

async fn addressbook_report(
    pool: &sqlx::PgPool,
    user: &str,
    book: &str,
    book_id: i64,
    body: &str,
) -> Response {
    let is_multiget = body.contains("addressbook-multiget");

    if is_multiget {
        let contacts = extract_multiget_contacts(pool, book_id, body).await;
        let mut responses = String::new();
        for (uid, etag, vcard) in &contacts {
            let contact_href = format!("/dav/contacts/{user}/{book}/{uid}.vcf");
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:getetag>\"{etag}\"</D:getetag>\n\
                 <CR:address-data>{}</CR:address-data>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&contact_href),
                xml_escape(vcard),
            ));
        }
        multistatus(&responses)
    } else {
        // addressbook-query: return all
        let contacts = sqlx::query_as::<_, (String, String, String)>(
            "SELECT uid, etag, vcard FROM contacts WHERE address_book_id = $1",
        )
        .bind(book_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut responses = String::new();
        for (uid, etag, vcard) in &contacts {
            let contact_href = format!("/dav/contacts/{user}/{book}/{uid}.vcf");
            responses.push_str(&format!(
                "<D:response>\n\
                 <D:href>{}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:getetag>\"{etag}\"</D:getetag>\n\
                 <CR:address-data>{}</CR:address-data>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&contact_href),
                xml_escape(vcard),
            ));
        }
        multistatus(&responses)
    }
}

async fn extract_multiget_contacts(
    pool: &sqlx::PgPool,
    book_id: i64,
    body: &str,
) -> Vec<(String, String, String)> {
    let mut uids = Vec::new();
    for href_start in body.match_indices("<D:href>").map(|(i, _)| i) {
        let rest = &body[href_start + 8..];
        if let Some(end) = rest.find("</D:href>") {
            let href = rest[..end].trim();
            if let Some(uid) = href.strip_suffix(".vcf").and_then(|s| s.rsplit('/').next()) {
                uids.push(uid.to_string());
            }
        }
    }
    for href_start in body.match_indices("<href>").map(|(i, _)| i) {
        let rest = &body[href_start + 6..];
        if let Some(end) = rest.find("</href>") {
            let href = rest[..end].trim();
            if let Some(uid) = href.strip_suffix(".vcf").and_then(|s| s.rsplit('/').next()) {
                uids.push(uid.to_string());
            }
        }
    }

    if uids.is_empty() {
        return Vec::new();
    }

    let placeholders: Vec<String> = uids.iter().enumerate().map(|(i, _)| format!("${}", i + 2)).collect();
    let query = format!(
        "SELECT uid, etag, vcard FROM contacts WHERE address_book_id = $1 AND uid IN ({})",
        placeholders.join(", ")
    );

    let mut q = sqlx::query_as::<_, (String, String, String)>(&query).bind(book_id);
    for uid in &uids {
        q = q.bind(uid);
    }
    q.fetch_all(pool).await.unwrap_or_default()
}

// ── contact resource (/dav/contacts/{user}/{book}/{uid}) ─────────────

pub(super) async fn dav_contact(
    method: Method,
    Path((user, book, uid_file)): Path<(String, String, String)>,
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if method == Method::OPTIONS {
        return options_response();
    }

    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }

    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let book_name = urlencoding::decode(&book).unwrap_or_default().into_owned();
    let uid = uid_file.strip_suffix(".vcf").unwrap_or(&uid_file);

    let book_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM address_books WHERE account_address = $1 AND name = $2",
    )
    .bind(&address)
    .bind(&book_name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some(book_id) = book_id else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match method {
        Method::GET => contact_get(pool, uid, book_id).await,
        Method::PUT => contact_put(pool, uid, book_id, &headers, &body).await,
        Method::DELETE => contact_delete(pool, uid, book_id).await,
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn contact_get(pool: &sqlx::PgPool, uid: &str, book_id: i64) -> Response {
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT etag, vcard FROM contacts WHERE address_book_id = $1 AND uid = $2",
    )
    .bind(book_id)
    .bind(uid)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    match row {
        Some((etag, vcard)) => Response::builder()
            .status(200)
            .header("content-type", "text/vcard; charset=utf-8")
            .header("etag", format!("\"{etag}\""))
            .body(axum::body::Body::from(vcard))
            .unwrap(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn contact_put(
    pool: &sqlx::PgPool,
    uid: &str,
    book_id: i64,
    headers: &HeaderMap,
    body: &str,
) -> Response {
    // if-match check
    if let Some(if_match) = headers.get("if-match").and_then(|v| v.to_str().ok()) {
        let expected = if_match.trim_matches('"');
        let current: Option<String> = sqlx::query_scalar(
            "SELECT etag FROM contacts WHERE address_book_id = $1 AND uid = $2",
        )
        .bind(book_id)
        .bind(uid)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        match current {
            Some(ref current_etag) if current_etag != expected => {
                return StatusCode::PRECONDITION_FAILED.into_response();
            }
            None => {
                return StatusCode::PRECONDITION_FAILED.into_response();
            }
            _ => {}
        }
    }

    // if-none-match: * means create-only
    if let Some(if_none_match) = headers.get("if-none-match").and_then(|v| v.to_str().ok())
        && if_none_match.trim() == "*" {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM contacts WHERE address_book_id = $1 AND uid = $2)",
            )
            .bind(book_id)
            .bind(uid)
            .fetch_one(pool)
            .await
            .unwrap_or(false);
            if exists {
                return StatusCode::PRECONDITION_FAILED.into_response();
            }
        }

    let etag = etag_of(body);
    let fn_name = extract_vcard_field(body, "FN");
    let email = extract_vcard_field(body, "EMAIL");

    let result = sqlx::query(
        "INSERT INTO contacts (address_book_id, uid, etag, vcard, fn_name, email)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (address_book_id, uid)
         DO UPDATE SET etag = $3, vcard = $4, fn_name = $5, email = $6, updated_at = now()",
    )
    .bind(book_id)
    .bind(uid)
    .bind(&etag)
    .bind(body)
    .bind(&fn_name)
    .bind(&email)
    .execute(pool)
    .await;

    match result {
        Ok(_) => Response::builder()
            .status(StatusCode::CREATED)
            .header("etag", format!("\"{etag}\""))
            .body(axum::body::Body::empty())
            .unwrap(),
        Err(e) => {
            tracing::error!("dav contact put error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn contact_delete(pool: &sqlx::PgPool, uid: &str, book_id: i64) -> Response {
    let result = sqlx::query(
        "DELETE FROM contacts WHERE address_book_id = $1 AND uid = $2",
    )
    .bind(book_id)
    .bind(uid)
    .execute(pool)
    .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("dav contact delete error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ── iCalendar/vCard field extraction ─────────────────────────────────

fn extract_ical_field(ical: &str, field: &str) -> String {
    for line in ical.lines() {
        // handle both "FIELD:value" and "FIELD;params:value"
        if let Some(rest) = line.strip_prefix(field) {
            if let Some(value) = rest.strip_prefix(':') {
                return value.trim().to_string();
            }
            if rest.starts_with(';')
                && let Some(pos) = rest.find(':') {
                    return rest[pos + 1..].trim().to_string();
                }
        }
    }
    String::new()
}

fn extract_ical_datetime(ical: &str, field: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let value = extract_ical_field(ical, field);
    if value.is_empty() {
        return None;
    }

    // try formats: 20240101T120000Z, 20240101T120000, 20240101
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ") {
        return Some(dt.and_utc());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%S") {
        return Some(dt.and_utc());
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(&value, "%Y%m%d") {
        return d
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc());
    }
    None
}

fn extract_vcard_field(vcard: &str, field: &str) -> String {
    for line in vcard.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            if let Some(value) = rest.strip_prefix(':') {
                return value.trim().to_string();
            }
            if rest.starts_with(';')
                && let Some(pos) = rest.find(':') {
                    return rest[pos + 1..].trim().to_string();
                }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_etag_of() {
        let etag = etag_of("hello world");
        assert_eq!(etag.len(), 16);
        // deterministic
        assert_eq!(etag, etag_of("hello world"));
        // different content -> different etag
        assert_ne!(etag, etag_of("hello world!"));
    }

    #[test]
    fn test_xml_escape() {
        assert_eq!(xml_escape("a<b>c&d\"e"), "a&lt;b&gt;c&amp;d&quot;e");
        assert_eq!(xml_escape("plain"), "plain");
    }

    #[test]
    fn test_extract_ical_field_simple() {
        let ical = "BEGIN:VEVENT\nSUMMARY:Team Meeting\nEND:VEVENT";
        assert_eq!(extract_ical_field(ical, "SUMMARY"), "Team Meeting");
    }

    #[test]
    fn test_extract_ical_field_with_params() {
        let ical = "BEGIN:VEVENT\nDTSTART;TZID=US/Eastern:20240101T120000\nEND:VEVENT";
        assert_eq!(
            extract_ical_field(ical, "DTSTART"),
            "20240101T120000"
        );
    }

    #[test]
    fn test_extract_ical_field_missing() {
        let ical = "BEGIN:VEVENT\nSUMMARY:Test\nEND:VEVENT";
        assert_eq!(extract_ical_field(ical, "DESCRIPTION"), "");
    }

    #[test]
    fn test_extract_ical_datetime_utc() {
        let ical = "DTSTART:20240315T100000Z";
        let dt = extract_ical_datetime(ical, "DTSTART").unwrap();
        assert_eq!(dt.to_rfc3339(), "2024-03-15T10:00:00+00:00");
    }

    #[test]
    fn test_extract_ical_datetime_local() {
        let ical = "DTSTART:20240315T100000";
        let dt = extract_ical_datetime(ical, "DTSTART").unwrap();
        assert_eq!(dt.to_rfc3339(), "2024-03-15T10:00:00+00:00");
    }

    #[test]
    fn test_extract_ical_datetime_date_only() {
        let ical = "DTSTART;VALUE=DATE:20240315";
        let dt = extract_ical_datetime(ical, "DTSTART").unwrap();
        assert_eq!(dt.to_rfc3339(), "2024-03-15T00:00:00+00:00");
    }

    #[test]
    fn test_extract_ical_datetime_missing() {
        let ical = "SUMMARY:Test";
        assert!(extract_ical_datetime(ical, "DTSTART").is_none());
    }

    #[test]
    fn test_extract_vcard_field() {
        let vcard = "BEGIN:VCARD\nVERSION:3.0\nFN:John Doe\nEMAIL:john@example.com\nEND:VCARD";
        assert_eq!(extract_vcard_field(vcard, "FN"), "John Doe");
        assert_eq!(extract_vcard_field(vcard, "EMAIL"), "john@example.com");
    }

    #[test]
    fn test_extract_vcard_field_with_params() {
        let vcard = "EMAIL;TYPE=WORK:john@company.com";
        assert_eq!(extract_vcard_field(vcard, "EMAIL"), "john@company.com");
    }

    #[test]
    fn test_extract_vcard_field_missing() {
        let vcard = "BEGIN:VCARD\nFN:Test\nEND:VCARD";
        assert_eq!(extract_vcard_field(vcard, "TEL"), "");
    }
}

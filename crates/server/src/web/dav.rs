//! Axum wrapper around `mailrs-dav`.
//!
//! All the protocol-level CalDAV / CardDAV plumbing (PROPFIND / REPORT /
//! multistatus / multiget / etag / preconditions / iCalendar + vCard
//! extraction) lives in the standalone `mailrs-dav` crate. This file is the
//! thin adapter that:
//!
//! 1. Implements `mailrs_dav::CalendarStore` + `AddressBookStore` against
//!    mailrs's PostgreSQL schema (`calendars`, `calendar_events`,
//!    `address_books`, `contacts`).
//! 2. Wraps that in axum-facing handler fns the router registers.
//! 3. Handles auth (Basic / Bearer via `WebState`) — that part stays in the
//!    server because it depends on `WebState.domain_store` and `ldap_config`.
//!
//! The structured-iTIP path (`mailrs_ical::parse_invite` + `crate::calendar::
//! upsert_from_parsed_invite`) is preserved inside `put_event`, so calendar
//! invite reconciliation continues to work after the extraction.

use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};

use mailrs_dav::error::DavError;
use mailrs_dav::parse::parse_depth;
use mailrs_dav::store::{AddressBookStore, CalendarStore, StoreError};
use mailrs_dav::types::{AddressBook, Calendar, Contact, Event, PutResult};
use mailrs_dav::xml::{DavResponse, options_response};
use mailrs_dav::{caldav, carddav, principal};

use super::WebState;

// ── DavResponse → axum::Response ─────────────────────────────────────

fn to_axum(resp: DavResponse) -> Response {
    let mut builder =
        Response::builder().status(StatusCode::from_u16(resp.status).unwrap_or(StatusCode::OK));
    for (k, v) in &resp.headers {
        builder = builder.header(k, v);
    }
    builder
        .body(axum::body::Body::from(resp.body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn err_to_axum(err: DavError) -> Response {
    to_axum(err.to_dav_response())
}

// ── auth ─────────────────────────────────────────────────────────────

fn require_basic_auth() -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header("www-authenticate", "Basic realm=\"mailrs\"")
        .body(axum::body::Body::from("authentication required"))
        .unwrap()
}

/// extract account address from HTTP Basic / Bearer auth, verifying credentials.
async fn authenticate(headers: &HeaderMap, state: &Arc<WebState>) -> Option<String> {
    let auth_header = headers.get("authorization")?.to_str().ok()?;

    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        if let Some(session) = state.sessions.get(token)
            && session.created_at.elapsed() < super::SESSION_TTL
        {
            return Some(session.address.clone());
        }
        return None;
    }

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
    } else if let Some(ref ldap) = state.ldap_config
        && ldap.authenticate(username, password).await
    {
        Some(account.address)
    } else {
        None
    }
}

fn depth_from_headers(headers: &HeaderMap) -> u32 {
    parse_depth(headers.get("depth").and_then(|v| v.to_str().ok()))
}

// ── DavAdapter — sqlx-backed store impl ──────────────────────────────

#[derive(Clone)]
struct DavAdapter {
    pool: sqlx::PgPool,
}

impl DavAdapter {
    fn from_state(state: &Arc<WebState>) -> Option<Self> {
        Some(Self {
            pool: state.pg_pool.as_ref()?.clone(),
        })
    }
}

fn to_store_err(e: sqlx::Error) -> StoreError {
    Box::new(e)
}

#[async_trait]
impl CalendarStore for DavAdapter {
    async fn list_calendars(&self, user: &str) -> Result<Vec<Calendar>, StoreError> {
        let rows = sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT id, name, color, description FROM calendars \
             WHERE account_address = $1 ORDER BY name",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, name, color, description)| Calendar {
                id,
                name,
                color,
                description,
            })
            .collect())
    }

    async fn get_calendar(
        &self,
        user: &str,
        calendar_name: &str,
    ) -> Result<Option<Calendar>, StoreError> {
        let row = sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT id, name, color, description FROM calendars \
             WHERE account_address = $1 AND name = $2",
        )
        .bind(user)
        .bind(calendar_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(row.map(|(id, name, color, description)| Calendar {
            id,
            name,
            color,
            description,
        }))
    }

    async fn list_events(&self, calendar_id: i64) -> Result<Vec<Event>, StoreError> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT uid, etag, icalendar FROM calendar_events WHERE calendar_id = $1",
        )
        .bind(calendar_id)
        .fetch_all(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(rows
            .into_iter()
            .map(|(uid, etag, icalendar)| Event {
                uid,
                etag,
                icalendar,
                summary: String::new(),
                dtstart: None,
                dtend: None,
            })
            .collect())
    }

    async fn get_event(
        &self,
        calendar_id: i64,
        uid: &str,
    ) -> Result<Option<Event>, StoreError> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT etag, icalendar FROM calendar_events \
             WHERE calendar_id = $1 AND uid = $2",
        )
        .bind(calendar_id)
        .bind(uid)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(row.map(|(etag, icalendar)| Event {
            uid: uid.to_string(),
            etag,
            icalendar,
            summary: String::new(),
            dtstart: None,
            dtend: None,
        }))
    }

    async fn event_etag(
        &self,
        calendar_id: i64,
        uid: &str,
    ) -> Result<Option<String>, StoreError> {
        let etag: Option<String> = sqlx::query_scalar(
            "SELECT etag FROM calendar_events WHERE calendar_id = $1 AND uid = $2",
        )
        .bind(calendar_id)
        .bind(uid)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(etag)
    }

    async fn put_event(
        &self,
        calendar_id: i64,
        uid: &str,
        icalendar: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        let existed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM calendar_events WHERE calendar_id = $1 AND uid = $2)",
        )
        .bind(calendar_id)
        .bind(uid)
        .fetch_one(&self.pool)
        .await
        .map_err(to_store_err)?;

        // Prefer the structured iTIP-aware path (MRS-3): parse via `mailrs_ical`,
        // project all RFC 5545 / 5546 fields. Falls back to the legacy minimal
        // path for non-VEVENT objects or parser failure — those still write
        // raw + summary so the CalDAV GET round-trip works.
        let parsed = if icalendar.contains("BEGIN:VEVENT") {
            mailrs_ical::parse_invite(icalendar.as_bytes()).ok()
        } else {
            None
        };

        if let Some(ref parsed) = parsed {
            crate::calendar::upsert_from_parsed_invite(
                &self.pool,
                calendar_id,
                uid,
                parsed,
                icalendar,
                etag,
            )
            .await
            .map_err(to_store_err)?;
        } else {
            let summary = mailrs_dav::parse::extract_ical_field(icalendar, "SUMMARY");
            let dtstart = mailrs_dav::parse::extract_ical_datetime(icalendar, "DTSTART");
            let dtend = mailrs_dav::parse::extract_ical_datetime(icalendar, "DTEND");
            sqlx::query(
                "INSERT INTO calendar_events (calendar_id, uid, etag, icalendar, summary, dtstart, dtend)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (calendar_id, uid)
                 WHERE recurrence_id IS NULL
                 DO UPDATE SET etag = $3, icalendar = $4, summary = $5, dtstart = $6, dtend = $7, updated_at = now()",
            )
            .bind(calendar_id)
            .bind(uid)
            .bind(etag)
            .bind(icalendar)
            .bind(&summary)
            .bind(dtstart)
            .bind(dtend)
            .execute(&self.pool)
            .await
            .map_err(to_store_err)?;
        }

        Ok(PutResult {
            created: !existed,
            etag: etag.to_string(),
        })
    }

    async fn delete_event(
        &self,
        calendar_id: i64,
        uid: &str,
    ) -> Result<bool, StoreError> {
        let res = sqlx::query("DELETE FROM calendar_events WHERE calendar_id = $1 AND uid = $2")
            .bind(calendar_id)
            .bind(uid)
            .execute(&self.pool)
            .await
            .map_err(to_store_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn ensure_default_calendar(&self, user: &str) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO calendars (account_address, name) VALUES ($1, 'Default') ON CONFLICT DO NOTHING",
        )
        .bind(user)
        .execute(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(())
    }
}

#[async_trait]
impl AddressBookStore for DavAdapter {
    async fn list_address_books(&self, user: &str) -> Result<Vec<AddressBook>, StoreError> {
        let rows = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT id, name, description FROM address_books \
             WHERE account_address = $1 ORDER BY name",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, name, description)| AddressBook {
                id,
                name,
                description,
            })
            .collect())
    }

    async fn get_address_book(
        &self,
        user: &str,
        book_name: &str,
    ) -> Result<Option<AddressBook>, StoreError> {
        let row = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT id, name, description FROM address_books \
             WHERE account_address = $1 AND name = $2",
        )
        .bind(user)
        .bind(book_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(row.map(|(id, name, description)| AddressBook {
            id,
            name,
            description,
        }))
    }

    async fn list_contacts(&self, book_id: i64) -> Result<Vec<Contact>, StoreError> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT uid, etag, vcard FROM contacts WHERE address_book_id = $1",
        )
        .bind(book_id)
        .fetch_all(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(rows
            .into_iter()
            .map(|(uid, etag, vcard)| Contact {
                uid,
                etag,
                vcard,
                fn_name: String::new(),
                email: String::new(),
            })
            .collect())
    }

    async fn get_contact(
        &self,
        book_id: i64,
        uid: &str,
    ) -> Result<Option<Contact>, StoreError> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT etag, vcard FROM contacts WHERE address_book_id = $1 AND uid = $2",
        )
        .bind(book_id)
        .bind(uid)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(row.map(|(etag, vcard)| Contact {
            uid: uid.to_string(),
            etag,
            vcard,
            fn_name: String::new(),
            email: String::new(),
        }))
    }

    async fn contact_etag(
        &self,
        book_id: i64,
        uid: &str,
    ) -> Result<Option<String>, StoreError> {
        let etag: Option<String> = sqlx::query_scalar(
            "SELECT etag FROM contacts WHERE address_book_id = $1 AND uid = $2",
        )
        .bind(book_id)
        .bind(uid)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(etag)
    }

    async fn put_contact(
        &self,
        book_id: i64,
        uid: &str,
        vcard: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        let existed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE address_book_id = $1 AND uid = $2)",
        )
        .bind(book_id)
        .bind(uid)
        .fetch_one(&self.pool)
        .await
        .map_err(to_store_err)?;

        let fn_name = mailrs_dav::parse::extract_vcard_field(vcard, "FN");
        let email = mailrs_dav::parse::extract_vcard_field(vcard, "EMAIL");

        sqlx::query(
            "INSERT INTO contacts (address_book_id, uid, etag, vcard, fn_name, email)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (address_book_id, uid)
             DO UPDATE SET etag = $3, vcard = $4, fn_name = $5, email = $6, updated_at = now()",
        )
        .bind(book_id)
        .bind(uid)
        .bind(etag)
        .bind(vcard)
        .bind(&fn_name)
        .bind(&email)
        .execute(&self.pool)
        .await
        .map_err(to_store_err)?;

        Ok(PutResult {
            created: !existed,
            etag: etag.to_string(),
        })
    }

    async fn delete_contact(&self, book_id: i64, uid: &str) -> Result<bool, StoreError> {
        let res = sqlx::query("DELETE FROM contacts WHERE address_book_id = $1 AND uid = $2")
            .bind(book_id)
            .bind(uid)
            .execute(&self.pool)
            .await
            .map_err(to_store_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn ensure_default_address_book(&self, user: &str) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO address_books (account_address, name) VALUES ($1, 'Default') ON CONFLICT DO NOTHING",
        )
        .bind(user)
        .execute(&self.pool)
        .await
        .map_err(to_store_err)?;
        Ok(())
    }
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
        return to_axum(options_response());
    }
    if method.as_str() != "PROPFIND" {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };

    let Some(adapter) = DavAdapter::from_state(&state) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    // Eager-create defaults so first-login CalDAV / CardDAV clients see
    // something to subscribe to. Errors are non-fatal — the propfind still
    // returns home-set hrefs.
    let _ = adapter.ensure_default_calendar(&address).await;
    let _ = adapter.ensure_default_address_book(&address).await;

    match principal::principal_propfind(&address, &body) {
        Ok(resp) => to_axum(resp),
        Err(e) => err_to_axum(e),
    }
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
        return to_axum(options_response());
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
    let Some(adapter) = DavAdapter::from_state(&state) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let depth = depth_from_headers(&headers);
    match caldav::calendar_home_propfind(&adapter, &user, depth).await {
        Ok(resp) => to_axum(resp),
        Err(e) => err_to_axum(e),
    }
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
        return to_axum(options_response());
    }
    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(adapter) = DavAdapter::from_state(&state) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let calendar_name = urlencoding::decode(&calendar)
        .unwrap_or_default()
        .into_owned();
    let cal = match <DavAdapter as CalendarStore>::get_calendar(&adapter, &address, &calendar_name)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("dav calendar lookup: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let depth = depth_from_headers(&headers);
    let result = match method.as_str() {
        "PROPFIND" => caldav::calendar_propfind(&adapter, &user, &calendar, cal.id, depth).await,
        "REPORT" => caldav::calendar_report(&adapter, &user, &calendar, cal.id, &body).await,
        _ => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    match result {
        Ok(resp) => to_axum(resp),
        Err(e) => err_to_axum(e),
    }
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
        return to_axum(options_response());
    }
    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(adapter) = DavAdapter::from_state(&state) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let calendar_name = urlencoding::decode(&calendar)
        .unwrap_or_default()
        .into_owned();
    let uid = uid_file.strip_suffix(".ics").unwrap_or(&uid_file);

    let cal = match <DavAdapter as CalendarStore>::get_calendar(&adapter, &address, &calendar_name)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("dav calendar lookup: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let if_match = headers.get("if-match").and_then(|v| v.to_str().ok());
    let if_none_match = headers.get("if-none-match").and_then(|v| v.to_str().ok());

    let result = match method {
        Method::GET => caldav::event_get(&adapter, cal.id, uid).await,
        Method::PUT => {
            caldav::event_put(&adapter, cal.id, uid, if_match, if_none_match, &body).await
        }
        Method::DELETE => caldav::event_delete(&adapter, cal.id, uid).await,
        _ => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    match result {
        Ok(resp) => to_axum(resp),
        Err(e) => err_to_axum(e),
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
        return to_axum(options_response());
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
    let Some(adapter) = DavAdapter::from_state(&state) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let depth = depth_from_headers(&headers);
    match carddav::addressbook_home_propfind(&adapter, &user, depth).await {
        Ok(resp) => to_axum(resp),
        Err(e) => err_to_axum(e),
    }
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
        return to_axum(options_response());
    }
    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(adapter) = DavAdapter::from_state(&state) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let book_name = urlencoding::decode(&book).unwrap_or_default().into_owned();
    let ab = match <DavAdapter as AddressBookStore>::get_address_book(
        &adapter, &address, &book_name,
    )
    .await
    {
        Ok(Some(b)) => b,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("dav address book lookup: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let depth = depth_from_headers(&headers);
    let result = match method.as_str() {
        "PROPFIND" => carddav::addressbook_propfind(&adapter, &user, &book, ab.id, depth).await,
        "REPORT" => carddav::addressbook_report(&adapter, &user, &book, ab.id, &body).await,
        _ => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    match result {
        Ok(resp) => to_axum(resp),
        Err(e) => err_to_axum(e),
    }
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
        return to_axum(options_response());
    }
    let Some(address) = authenticate(&headers, &state).await else {
        return require_basic_auth();
    };
    if address != user {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(adapter) = DavAdapter::from_state(&state) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let book_name = urlencoding::decode(&book).unwrap_or_default().into_owned();
    let uid = uid_file.strip_suffix(".vcf").unwrap_or(&uid_file);

    let ab = match <DavAdapter as AddressBookStore>::get_address_book(
        &adapter, &address, &book_name,
    )
    .await
    {
        Ok(Some(b)) => b,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("dav address book lookup: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let if_match = headers.get("if-match").and_then(|v| v.to_str().ok());
    let if_none_match = headers.get("if-none-match").and_then(|v| v.to_str().ok());

    let result = match method {
        Method::GET => carddav::contact_get(&adapter, ab.id, uid).await,
        Method::PUT => {
            carddav::contact_put(&adapter, ab.id, uid, if_match, if_none_match, &body).await
        }
        Method::DELETE => carddav::contact_delete(&adapter, ab.id, uid).await,
        _ => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    match result {
        Ok(resp) => to_axum(resp),
        Err(e) => err_to_axum(e),
    }
}

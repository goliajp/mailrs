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

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use mailrs_dav::error::DavError;
use mailrs_dav::parse::parse_depth;
use mailrs_dav::store::StoreError;
use mailrs_dav::xml::DavResponse;

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

// ── submodules with the trait impls + axum handlers ───────────────────

mod store_cal;
mod store_card;
mod handlers;

pub(crate) use handlers::{
    dav_calendar_collection, dav_calendar_home, dav_contact, dav_contact_collection,
    dav_contact_home, dav_event, dav_principal, well_known_caldav, well_known_carddav,
};

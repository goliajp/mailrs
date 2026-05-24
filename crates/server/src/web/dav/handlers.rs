//! Axum handler fns wired into the router. Auth + `DavAdapter`
//! construction + dispatch into the relevant `mailrs_dav` entry
//! points.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};

use mailrs_dav::store::{AddressBookStore, CalendarStore};
use mailrs_dav::xml::options_response;
use mailrs_dav::{caldav, carddav, principal};

use super::super::WebState;
use super::{
    DavAdapter, authenticate, depth_from_headers, err_to_axum, require_basic_auth, to_axum,
};


// ── well-known redirects ─────────────────────────────────────────────

pub(crate) async fn well_known_caldav() -> Redirect {
    Redirect::permanent("/dav/")
}

pub(crate) async fn well_known_carddav() -> Redirect {
    Redirect::permanent("/dav/")
}

// ── principal (/dav/) ────────────────────────────────────────────────

pub(crate) async fn dav_principal(
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

pub(crate) async fn dav_calendar_home(
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

pub(crate) async fn dav_calendar_collection(
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

pub(crate) async fn dav_event(
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

pub(crate) async fn dav_contact_home(
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

pub(crate) async fn dav_contact_collection(
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

pub(crate) async fn dav_contact(
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

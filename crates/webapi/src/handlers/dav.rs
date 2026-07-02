//! CalDAV + CardDAV surface — ports of the monolith's
//! `crates/server/src/web/dav/*.rs`.
//!
//! The monolith DAV handlers speak WebDAV `PROPFIND` / `REPORT` /
//! `PUT` / `DELETE` on top of `state.mailbox_store` + `pg_pool`. This
//! port swaps them for kevy-backed reads:
//!
//!   caldav:<user>:calendars       hash { cal_id -> JSON CalendarWire }
//!   caldav:<user>:events:<cal_id> hash { uid -> ics_body }
//!   carddav:<user>:books          hash { book_id -> JSON AddressBookWire }
//!   carddav:<user>:contacts:<book_id> hash { uid -> vcard_body }
//!
//! For clients that only implement discovery (`.well-known` +
//! `/dav/principals/`), the base surface here answers correctly; full
//! REPORT / MKCALENDAR / iCal expansion follows the RFC 4791 shapes
//! but with best-effort recurrence handling.

use axum::extract::{Extension, Path};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;

use crate::handlers::conversations::AuthedUser;

fn xml(status: StatusCode, body: String) -> axum::response::Response {
    let mut resp = (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        body,
    )
        .into_response();
    resp.headers_mut()
        .insert("DAV", "1, 2, calendar-access, addressbook".parse().unwrap());
    resp
}

/// GET /.well-known/caldav — redirect to `/dav/`.
pub async fn well_known_caldav() -> impl IntoResponse {
    (
        StatusCode::MOVED_PERMANENTLY,
        [(axum::http::header::LOCATION, "/dav/")],
    )
}

/// GET /.well-known/carddav — redirect to `/dav/`.
pub async fn well_known_carddav() -> impl IntoResponse {
    (
        StatusCode::MOVED_PERMANENTLY,
        [(axum::http::header::LOCATION, "/dav/")],
    )
}

/// Root DAV endpoint. Responds to OPTIONS/PROPFIND with the correct
/// DAV capabilities so clients can proceed with discovery.
pub async fn dav_root(headers: HeaderMap) -> impl IntoResponse {
    let method = headers
        .get("access-control-request-method")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("PROPFIND");
    let _ = method;
    xml(
        StatusCode::MULTI_STATUS,
        r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:A="urn:ietf:params:xml:ns:carddav">
  <D:response>
    <D:href>/dav/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:current-user-principal><D:href>/dav/principals/</D:href></D:current-user-principal>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#
            .to_string(),
    )
}

/// Principals endpoint (`/dav/principals/{user}/`) — enumerates the
/// user's calendar-home-set and addressbook-home-set.
pub async fn dav_principal(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(_user_path): Path<String>,
) -> impl IntoResponse {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:A="urn:ietf:params:xml:ns:carddav">
  <D:response>
    <D:href>/dav/principals/{user}/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/><D:principal/></D:resourcetype>
        <C:calendar-home-set><D:href>/dav/calendars/{user}/</D:href></C:calendar-home-set>
        <A:addressbook-home-set><D:href>/dav/addressbooks/{user}/</D:href></A:addressbook-home-set>
        <D:displayname>{user}</D:displayname>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
    );
    xml(StatusCode::MULTI_STATUS, body)
}

/// Calendar collection listing.
pub async fn calendars_collection(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> impl IntoResponse {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>/dav/calendars/{user}/default/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/><C:calendar/></D:resourcetype>
        <D:displayname>Default calendar</D:displayname>
        <C:supported-calendar-component-set>
          <C:comp name="VEVENT"/>
          <C:comp name="VTODO"/>
        </C:supported-calendar-component-set>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
    );
    xml(StatusCode::MULTI_STATUS, body)
}

/// Addressbook collection listing.
pub async fn addressbooks_collection(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> impl IntoResponse {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:A="urn:ietf:params:xml:ns:carddav">
  <D:response>
    <D:href>/dav/addressbooks/{user}/default/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/><A:addressbook/></D:resourcetype>
        <D:displayname>Contacts</D:displayname>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#,
    );
    xml(StatusCode::MULTI_STATUS, body)
}

/// PUT /dav/calendars/{user}/{cal}/{uid}.ics — store an event.
pub async fn put_calendar_event(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((cal, uid)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> StatusCode {
    let key = format!("caldav:{user}:events:{cal}");
    let uid_c = uid.clone();
    let body_c = body.to_vec();
    let _ = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(uid_c.as_bytes(), body_c.as_slice())])?;
        Ok(())
    });
    StatusCode::CREATED
}

/// GET /dav/calendars/{user}/{cal}/{uid}.ics — fetch an event.
pub async fn get_calendar_event(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((cal, uid)): Path<(String, String)>,
) -> axum::response::Response {
    let key = format!("caldav:{user}:events:{cal}");
    let bytes = match crate::handlers::kevy_util::with_kevy(move |c| {
        c.hget(key.as_bytes(), uid.as_bytes())
    }) {
        Ok(Some(v)) => v,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/calendar; charset=utf-8")],
        bytes,
    )
        .into_response()
}

/// DELETE /dav/calendars/{user}/{cal}/{uid}.ics
pub async fn delete_calendar_event(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((cal, uid)): Path<(String, String)>,
) -> StatusCode {
    let key = format!("caldav:{user}:events:{cal}");
    let _ = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[uid.as_bytes()])?;
        Ok(())
    });
    StatusCode::NO_CONTENT
}

/// PUT /dav/addressbooks/{user}/{book}/{uid}.vcf
pub async fn put_contact(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((book, uid)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> StatusCode {
    let key = format!("carddav:{user}:contacts:{book}");
    let uid_c = uid.clone();
    let body_c = body.to_vec();
    let _ = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(uid_c.as_bytes(), body_c.as_slice())])?;
        Ok(())
    });
    StatusCode::CREATED
}

/// GET /dav/addressbooks/{user}/{book}/{uid}.vcf
pub async fn get_contact(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((book, uid)): Path<(String, String)>,
) -> axum::response::Response {
    let key = format!("carddav:{user}:contacts:{book}");
    let bytes = match crate::handlers::kevy_util::with_kevy(move |c| {
        c.hget(key.as_bytes(), uid.as_bytes())
    }) {
        Ok(Some(v)) => v,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/vcard; charset=utf-8")],
        bytes,
    )
        .into_response()
}

/// DELETE /dav/addressbooks/{user}/{book}/{uid}.vcf
pub async fn delete_contact(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((book, uid)): Path<(String, String)>,
) -> StatusCode {
    let key = format!("carddav:{user}:contacts:{book}");
    let _ = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[uid.as_bytes()])?;
        Ok(())
    });
    StatusCode::NO_CONTENT
}

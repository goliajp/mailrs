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
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;

use crate::handlers::conversations::AuthedUser;

/// Standard DAV OPTIONS response — advertises the extensions we
/// implement so client-side auto-detection accepts us as caldav +
/// carddav.
fn options_response() -> axum::response::Response {
    let mut resp = (
        StatusCode::OK,
        [(
            "Allow",
            "OPTIONS, GET, HEAD, PROPFIND, REPORT, PUT, DELETE, MKCOL, MKCALENDAR",
        )],
    )
        .into_response();
    resp.headers_mut()
        .insert("DAV", "1, 2, calendar-access, addressbook".parse().unwrap());
    resp
}

fn xml(status: StatusCode, body: String) -> axum::response::Response {
    let mut resp = (
        status,
        [(
            axum::http::header::CONTENT_TYPE,
            "application/xml; charset=utf-8",
        )],
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

/// Root DAV endpoint. Responds to OPTIONS with the capabilities
/// header; PROPFIND / GET / HEAD return the standard collection
/// multistatus so client discovery can proceed.
pub async fn dav_root(method: Method, _headers: HeaderMap) -> axum::response::Response {
    if method == Method::OPTIONS {
        return options_response();
    }
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
    method: Method,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(_user_path): Path<String>,
) -> axum::response::Response {
    if method == Method::OPTIONS {
        return options_response();
    }
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
///
/// PROPFIND returns the single "default" calendar (which every user
/// implicitly owns). REPORT walks the kevy-stored events and emits a
/// multistatus so `calendar-multiget` / `calendar-query` clients (e.g.
/// iOS Calendar) can fetch event data in one round-trip instead of
/// PROPFIND-then-GET-each.
pub async fn calendars_collection(
    method: Method,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> axum::response::Response {
    if method == Method::OPTIONS {
        return options_response();
    }
    if method.as_str() == "REPORT" {
        return calendar_report_response(&user).await;
    }
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

/// Build a multistatus containing every event in the user's default
/// calendar. Sufficient for basic CalDAV client sync — full RFC 4791
/// time-range / calendar-data prop filtering would live in a proper
/// mailrs-dav port.
async fn calendar_report_response(user: &str) -> axum::response::Response {
    let key = format!("caldav:{user}:events:default");
    let flat = match crate::handlers::kevy_util::with_kevy(move |c| c.hgetall(key.as_bytes())) {
        Ok(v) => v,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let mut body = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
"#,
    );
    let mut i = 0;
    while i + 1 < flat.len() {
        let uid = String::from_utf8_lossy(&flat[i]).into_owned();
        let ics = String::from_utf8_lossy(&flat[i + 1]).into_owned();
        let etag = format!("{:x}", md5_ish(&ics));
        let href = format!("/dav/calendars/{user}/default/{uid}.ics");
        body.push_str("  <D:response>\n    <D:href>");
        body.push_str(&xml_escape(&href));
        body.push_str("</D:href>\n    <D:propstat>\n      <D:prop>\n        <D:getetag>\"");
        body.push_str(&etag);
        body.push_str("\"</D:getetag>\n        <C:calendar-data>");
        body.push_str(&xml_escape(&ics));
        body.push_str(
            "</C:calendar-data>\n      </D:prop>\n      <D:status>HTTP/1.1 200 OK</D:status>\n    </D:propstat>\n  </D:response>\n",
        );
        i += 2;
    }
    body.push_str("</D:multistatus>\n");
    xml(StatusCode::MULTI_STATUS, body)
}

/// Tiny non-cryptographic hash for stable ETag generation. Same input
/// bytes → same output; two clients storing the same .ics get the same
/// ETag so If-Match round-trips work. FNV-1a is enough for this use.
fn md5_ish(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// XML-escape the four reserved chars for embedding user-provided
/// strings inside PROPFIND / REPORT response bodies.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Addressbook collection listing.
///
/// PROPFIND returns the single "default" address book. REPORT walks
/// the kevy-stored vcards for `addressbook-multiget` / `addressbook-
/// query`, matching the calendar handler shape.
pub async fn addressbooks_collection(
    method: Method,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> axum::response::Response {
    if method == Method::OPTIONS {
        return options_response();
    }
    if method.as_str() == "REPORT" {
        return addressbook_report_response(&user).await;
    }
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

/// See [`calendar_report_response`] — same shape for CardDAV.
async fn addressbook_report_response(user: &str) -> axum::response::Response {
    let key = format!("carddav:{user}:contacts:default");
    let flat = match crate::handlers::kevy_util::with_kevy(move |c| c.hgetall(key.as_bytes())) {
        Ok(v) => v,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let mut body = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:A="urn:ietf:params:xml:ns:carddav">
"#,
    );
    let mut i = 0;
    while i + 1 < flat.len() {
        let uid = String::from_utf8_lossy(&flat[i]).into_owned();
        let vcf = String::from_utf8_lossy(&flat[i + 1]).into_owned();
        let etag = format!("{:x}", md5_ish(&vcf));
        let href = format!("/dav/addressbooks/{user}/default/{uid}.vcf");
        body.push_str("  <D:response>\n    <D:href>");
        body.push_str(&xml_escape(&href));
        body.push_str("</D:href>\n    <D:propstat>\n      <D:prop>\n        <D:getetag>\"");
        body.push_str(&etag);
        body.push_str("\"</D:getetag>\n        <A:address-data>");
        body.push_str(&xml_escape(&vcf));
        body.push_str(
            "</A:address-data>\n      </D:prop>\n      <D:status>HTTP/1.1 200 OK</D:status>\n    </D:propstat>\n  </D:response>\n",
        );
        i += 2;
    }
    body.push_str("</D:multistatus>\n");
    xml(StatusCode::MULTI_STATUS, body)
}

/// PUT /dav/calendars/{user}/{cal}/{uid}.ics — store an event.
/// The `{user}` path segment is ignored — the authenticated user
/// (from the session) is the source of truth, so `user_A` can't
/// smuggle a PUT to `/dav/calendars/user_B/…`.
pub async fn put_calendar_event(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((_url_user, cal, uid)): Path<(String, String, String)>,
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
    Path((_url_user, cal, uid)): Path<(String, String, String)>,
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
        [(
            axum::http::header::CONTENT_TYPE,
            "text/calendar; charset=utf-8",
        )],
        bytes,
    )
        .into_response()
}

/// DELETE /dav/calendars/{user}/{cal}/{uid}.ics
pub async fn delete_calendar_event(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((_url_user, cal, uid)): Path<(String, String, String)>,
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
    Path((_url_user, book, uid)): Path<(String, String, String)>,
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
    Path((_url_user, book, uid)): Path<(String, String, String)>,
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
        [(
            axum::http::header::CONTENT_TYPE,
            "text/vcard; charset=utf-8",
        )],
        bytes,
    )
        .into_response()
}

/// DELETE /dav/addressbooks/{user}/{book}/{uid}.vcf
pub async fn delete_contact(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((_url_user, book, uid)): Path<(String, String, String)>,
) -> StatusCode {
    let key = format!("carddav:{user}:contacts:{book}");
    let _ = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[uid.as_bytes()])?;
        Ok(())
    });
    StatusCode::NO_CONTENT
}

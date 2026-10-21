//! CalDAV + CardDAV: axum glue over `mailrs-dav`.
//!
//! The protocol (paths, PROPFIND / REPORT / GET / PUT / DELETE, ETags and
//! preconditions) is `mailrs_dav::dispatch`; the storage is
//! `mailrs_core_sidestate::families::dav_store`, over the same event and
//! contact rows the web reads. This file turns an axum request into a
//! `DavRequest` and a `DavResponse` back into an axum response.

use axum::body::Bytes;
use axum::extract::Extension;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use mailrs_core_sidestate::families::dav_store::{DavStore, KevyAccess};
use mailrs_dav::dispatch::{DavRequest, handle};
use mailrs_dav::{DavError, DavResponse};

use crate::handlers::conversations::AuthedUser;

/// The process's pooled network-kevy connections.
struct Pool;

impl KevyAccess for Pool {
    fn with_conn<T>(
        &self,
        f: impl FnOnce(&mut kevy_client::Connection) -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        crate::handlers::kevy_util::off_reactor(|| crate::kevy_pool::with_conn(f))
    }
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

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn to_axum(resp: DavResponse) -> Response {
    let mut builder = Response::builder().status(resp.status);
    for (k, v) in &resp.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    match builder.body(axum::body::Body::from(resp.body)) {
        Ok(out) => out,
        Err(e) => {
            tracing::error!(err = %e, "dav response could not be built");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Every method on `/dav/` and below.
pub async fn dav(
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    body: Bytes,
) -> Response {
    let Ok(body) = std::str::from_utf8(&body) else {
        return to_axum(DavError::BadRequest("body is not UTF-8".into()).to_dav_response());
    };
    let req = DavRequest {
        method: method.as_str(),
        path: uri.path(),
        depth: header(&headers, "depth"),
        if_match: header(&headers, "if-match"),
        if_none_match: header(&headers, "if-none-match"),
        body,
    };
    let store = DavStore::new(user.as_str(), Pool);
    match handle(&req, &user, &store, &store).await {
        Ok(resp) => to_axum(resp),
        Err(e) => {
            if let DavError::ServerError(detail) = &e {
                tracing::error!(%detail, method = %method, path = %uri.path(), "dav request failed");
            }
            to_axum(e.to_dav_response())
        }
    }
}

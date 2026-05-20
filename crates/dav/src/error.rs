//! CalDAV / CardDAV error variants used by handler return values.
//!
//! Each variant carries enough information to produce a meaningful HTTP
//! response via [`to_dav_response`](Self::to_dav_response), so server-side
//! wiring code can blanket-convert handler errors without inspecting them.

use crate::xml::DavResponse;

/// Errors a DAV handler can return.
///
/// Most variants map 1:1 to HTTP status codes; [`ServerError`](Self::ServerError)
/// is the catch-all for anything the store impl can't classify.
#[derive(Debug, Clone)]
pub enum DavError {
    /// 401 — auth required. Server-side wrapper is expected to add the
    /// `WWW-Authenticate` header.
    Unauthorized,
    /// 403 — authenticated but not permitted.
    Forbidden,
    /// 404 — resource doesn't exist.
    NotFound,
    /// 400 — malformed request body / unsupported parameters.
    BadRequest(String),
    /// 409 — server-side semantic conflict (e.g. parent collection missing).
    Conflict,
    /// 412 — `If-Match` / `If-None-Match` precondition failed.
    PreconditionFailed,
    /// 405 — verb not allowed on this resource.
    MethodNotAllowed,
    /// 503 — backing store unavailable (e.g. database down).
    ServiceUnavailable,
    /// 500 — anything else. Description is for the server log, not the client.
    ServerError(String),
}

impl DavError {
    /// Convert into the minimal `DavResponse` a server can serve directly.
    ///
    /// Bodies are short, plain-text, ASCII; suitable as a fallback when the
    /// server-side adapter doesn't want to do custom error formatting.
    pub fn to_dav_response(&self) -> DavResponse {
        let (status, body) = match self {
            DavError::Unauthorized => (401, "authentication required"),
            DavError::Forbidden => (403, "forbidden"),
            DavError::NotFound => (404, "not found"),
            DavError::BadRequest(_) => (400, "bad request"),
            DavError::Conflict => (409, "conflict"),
            DavError::PreconditionFailed => (412, "precondition failed"),
            DavError::MethodNotAllowed => (405, "method not allowed"),
            DavError::ServiceUnavailable => (503, "service unavailable"),
            DavError::ServerError(_) => (500, "internal server error"),
        };
        let mut resp = DavResponse::new(status).with_body(body.as_bytes().to_vec());
        if matches!(self, DavError::Unauthorized) {
            resp = resp.with_header("www-authenticate", "Basic realm=\"mailrs-dav\"");
        }
        resp
    }
}

impl std::fmt::Display for DavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DavError::Unauthorized => write!(f, "unauthorized"),
            DavError::Forbidden => write!(f, "forbidden"),
            DavError::NotFound => write!(f, "not found"),
            DavError::BadRequest(d) => write!(f, "bad request: {d}"),
            DavError::Conflict => write!(f, "conflict"),
            DavError::PreconditionFailed => write!(f, "precondition failed"),
            DavError::MethodNotAllowed => write!(f, "method not allowed"),
            DavError::ServiceUnavailable => write!(f, "service unavailable"),
            DavError::ServerError(d) => write!(f, "server error: {d}"),
        }
    }
}

impl std::error::Error for DavError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_response_has_auth_header() {
        let resp = DavError::Unauthorized.to_dav_response();
        assert_eq!(resp.status, 401);
        assert!(
            resp.headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("www-authenticate"))
        );
    }

    #[test]
    fn not_found_maps_to_404() {
        assert_eq!(DavError::NotFound.to_dav_response().status, 404);
    }

    #[test]
    fn precondition_failed_maps_to_412() {
        assert_eq!(DavError::PreconditionFailed.to_dav_response().status, 412);
    }

    #[test]
    fn server_error_does_not_leak_description() {
        // description is for logs; body must not include it
        let resp = DavError::ServerError("db borked".into()).to_dav_response();
        assert_eq!(resp.status, 500);
        assert!(!String::from_utf8_lossy(&resp.body).contains("borked"));
    }
}

//! Cross-site request forgery.
//!
//! A browser attaches the session cookie to any request it makes for this
//! origin, including one a page on another site starts. What such a page
//! cannot do is read our cookies or set a request header, so a request that
//! was authenticated by the cookie and changes something must carry the
//! token in `X-CSRF-Token`, and, when it says where it comes from, come from
//! one of our own web origins. A request authenticated by an
//! `Authorization` header (the apps, agent keys) is exempt: no cross-site
//! page can put one on a browser's request.
//!
//! The token is derived from the session token rather than stored with the
//! session. Without the session token it cannot be computed, so a cookie
//! planted from a sibling subdomain does not help; and sessions that existed
//! before this have one as soon as they are next used. The domain separator
//! keeps it apart from the session's own key in the store.

use std::sync::OnceLock;

use axum::http::{HeaderMap, HeaderValue, Method, header};
use sha2::{Digest, Sha256};

pub(crate) const CSRF_COOKIE: &str = "mailrs_csrf";
pub(crate) const CSRF_HEADER: &str = "x-csrf-token";

/// Why a request was refused. The code goes in the 403 body so the web
/// client can tell a missing token (re-read the cookie, retry once) from the
/// rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    Missing,
    Mismatch,
    Origin,
}

impl Violation {
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing => "csrf_missing",
            Self::Mismatch => "csrf_mismatch",
            Self::Origin => "origin_mismatch",
        }
    }
}

/// `MAILRS_CSRF`: `enforce` (the default) refuses a violation; `report`
/// only logs it. Report exists for the switch-over and has an end date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Enforce,
    Report,
}

pub fn mode_from(value: Option<&str>) -> Result<Mode, String> {
    match value {
        None | Some("enforce") => Ok(Mode::Enforce),
        Some("report") => Ok(Mode::Report),
        Some(other) => Err(format!(
            "MAILRS_CSRF={other:?}: expected \"enforce\" or \"report\""
        )),
    }
}

/// The web origins requests may come from, scheme and host and port
/// exactly: `MAILRS_WEB_ORIGINS` (comma separated) when set, otherwise
/// `https://` + `MAILRS_HOSTNAME`.
pub fn origins_from(web_origins: Option<&str>, hostname: Option<&str>) -> Vec<String> {
    if let Some(list) = web_origins {
        return list
            .split(',')
            .map(str::trim)
            .filter(|o| !o.is_empty())
            .map(String::from)
            .collect();
    }
    hostname
        .filter(|h| !h.is_empty())
        .map(|h| vec![format!("https://{h}")])
        .unwrap_or_default()
}

pub struct Config {
    pub mode: Mode,
    pub origins: Vec<String>,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

fn from_env() -> Result<Config, String> {
    let mode = mode_from(std::env::var("MAILRS_CSRF").ok().as_deref())?;
    let origins = origins_from(
        std::env::var("MAILRS_WEB_ORIGINS").ok().as_deref(),
        std::env::var("MAILRS_HOSTNAME").ok().as_deref(),
    );
    if origins.is_empty() {
        tracing::warn!(
            "csrf: no web origin configured; every request that names an origin is refused"
        );
    }
    Ok(Config { mode, origins })
}

/// Read once. `run` calls it at start so a bad `MAILRS_CSRF` stops the
/// process there; an unknown mode is never guessed.
pub fn config() -> &'static Config {
    CONFIG.get_or_init(|| from_env().unwrap_or_else(|e| panic!("{e}")))
}

/// The CSRF token for a session token.
pub fn token_for(session_token: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"mailrs-csrf\0");
    h.update(session_token.as_bytes());
    hex::encode(h.finalize())
}

fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// `Some(true)` when the request names one of our origins, `Some(false)`
/// when it names another (the literal `null` included), `None` when it
/// names none.
pub fn origin_ok(headers: &HeaderMap, origins: &[String]) -> Option<bool> {
    let origin = headers.get(header::ORIGIN)?;
    let origin = origin.to_str().unwrap_or("null");
    Some(origins.iter().any(|o| o == origin))
}

/// A refusal for a request that names another site's origin, for the
/// sign-in endpoints that have no session to check a token against. In
/// report mode it is only logged.
pub fn refuse_foreign_origin(headers: &HeaderMap, path: &str) -> Option<axum::response::Response> {
    use axum::response::IntoResponse;
    let cfg = config();
    if origin_ok(headers, &cfg.origins) != Some(false) {
        return None;
    }
    tracing::warn!(
        violation = Violation::Origin.code(),
        path,
        enforced = cfg.mode == Mode::Enforce,
        "csrf"
    );
    if cfg.mode == Mode::Report {
        return None;
    }
    Some(
        (
            axum::http::StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({ "error": Violation::Origin.code() })),
        )
            .into_response(),
    )
}

/// The check for a request authenticated by the session cookie.
pub fn check(
    method: &Method,
    headers: &HeaderMap,
    session_token: &str,
    origins: &[String],
) -> Result<(), Violation> {
    if is_safe(method) {
        return Ok(());
    }
    if origin_ok(headers, origins) == Some(false) {
        return Err(Violation::Origin);
    }
    let Some(sent) = headers.get(CSRF_HEADER).and_then(|v| v.to_str().ok()) else {
        return Err(Violation::Missing);
    };
    if !same(sent.as_bytes(), token_for(session_token).as_bytes()) {
        return Err(Violation::Mismatch);
    }
    Ok(())
}

/// Equal, in time that does not depend on where they first differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Whether the request already carries the right token cookie.
pub fn has_cookie(headers: &HeaderMap, session_token: &str) -> bool {
    crate::handlers::auth::cookie_value(headers, CSRF_COOKIE)
        .is_some_and(|v| same(v.as_bytes(), token_for(session_token).as_bytes()))
}

/// `Set-Cookie` for the token. Not HttpOnly: the web client reads it to
/// send it back in the header.
pub fn set_cookie(session_token: &str) -> HeaderValue {
    let v = format!(
        "{CSRF_COOKIE}={}; Path=/; Secure; SameSite=Lax; Max-Age=604800",
        token_for(session_token)
    );
    HeaderValue::from_str(&v).expect("hex token is a valid header value")
}

pub fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("mailrs_csrf=; Path=/; Secure; SameSite=Lax; Max-Age=0")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "a1b2c3";
    fn origins() -> Vec<String> {
        vec!["https://mail.golia.ai".into()]
    }
    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn a_read_needs_nothing() {
        assert_eq!(
            check(&Method::GET, &HeaderMap::new(), SESSION, &origins()),
            Ok(())
        );
    }

    #[test]
    fn a_write_needs_the_token_for_this_session() {
        let ok = token_for(SESSION);
        assert_eq!(
            check(&Method::POST, &HeaderMap::new(), SESSION, &origins()),
            Err(Violation::Missing)
        );
        assert_eq!(
            check(
                &Method::POST,
                &headers(&[(CSRF_HEADER, "nope")]),
                SESSION,
                &origins()
            ),
            Err(Violation::Mismatch)
        );
        // another session's token is not this one's
        assert_eq!(
            check(
                &Method::DELETE,
                &headers(&[(CSRF_HEADER, &token_for("other"))]),
                SESSION,
                &origins()
            ),
            Err(Violation::Mismatch)
        );
        assert_eq!(
            check(
                &Method::PUT,
                &headers(&[(CSRF_HEADER, &ok)]),
                SESSION,
                &origins()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_named_origin_must_be_ours_exactly() {
        let ok = token_for(SESSION);
        for bad in [
            "https://evil.example",
            "null",
            "http://mail.golia.ai",
            "https://mail.golia.ai:8443",
        ] {
            assert_eq!(
                check(
                    &Method::POST,
                    &headers(&[(CSRF_HEADER, &ok), ("origin", bad)]),
                    SESSION,
                    &origins()
                ),
                Err(Violation::Origin),
                "{bad}"
            );
        }
        assert_eq!(
            check(
                &Method::POST,
                &headers(&[(CSRF_HEADER, &ok), ("origin", "https://mail.golia.ai")]),
                SESSION,
                &origins()
            ),
            Ok(())
        );
    }

    #[test]
    fn the_token_is_not_the_session_and_not_its_plain_hash() {
        let t = token_for(SESSION);
        assert_ne!(t, SESSION);
        assert_ne!(t, hex::encode(Sha256::digest(SESSION.as_bytes())));
    }

    #[test]
    fn modes_and_origins_are_read_strictly() {
        assert_eq!(mode_from(None), Ok(Mode::Enforce));
        assert_eq!(mode_from(Some("report")), Ok(Mode::Report));
        assert!(mode_from(Some("off")).is_err());
        assert_eq!(origins_from(None, Some("mail.golia.ai")), origins());
        assert_eq!(
            origins_from(
                Some("http://localhost:6034, https://x.test"),
                Some("ignored")
            ),
            vec![
                "http://localhost:6034".to_string(),
                "https://x.test".to_string()
            ]
        );
        assert!(origins_from(None, None).is_empty());
    }
}

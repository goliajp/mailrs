//! The content security policy, report-only or enforced as
//! `MAILRS_CSP_MODE` says, and where browsers send what it blocked or
//! would have blocked.
//!
//! Mail is drawn into a shadow root of the page, so the page's policy is
//! the mail's policy too: styles inline in a message are allowed. Images
//! come only from this site — remote ones are the copies fastcore fetched
//! when the mail arrived. Scripts come only from this site.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Bytes;
use axum::http::{HeaderMap, HeaderName, StatusCode};

pub const POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; \
frame-src 'self' blob:; worker-src 'self'; manifest-src 'self'; object-src 'none'; \
base-uri 'none'; form-action 'self'; frame-ancestors 'none'; report-uri /api/csp-report";

/// Whether browsers only report what the policy would block, or block it.
/// Report-only until a week of real traffic reports nothing; the ops
/// daemon counts the reports on the monitor page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    ReportOnly,
    Enforce,
}

impl Mode {
    /// `MAILRS_CSP_MODE`: `report-only` (also when unset) or `enforce`.
    pub fn parse(value: Option<&str>) -> Result<Mode, String> {
        match value.map(str::trim) {
            None | Some("") | Some("report-only") => Ok(Mode::ReportOnly),
            Some("enforce") => Ok(Mode::Enforce),
            Some(other) => Err(format!(
                "MAILRS_CSP_MODE={other:?}: expected report-only or enforce"
            )),
        }
    }

    pub fn header(self) -> HeaderName {
        match self {
            Mode::ReportOnly => axum::http::header::CONTENT_SECURITY_POLICY_REPORT_ONLY,
            Mode::Enforce => axum::http::header::CONTENT_SECURITY_POLICY,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Mode::ReportOnly => "report-only",
            Mode::Enforce => "enforce",
        }
    }
}

static MODE: OnceLock<Mode> = OnceLock::new();

/// Read `MAILRS_CSP_MODE` and fix the mode for the life of the process.
/// Called at start, before the router is built; a value it does not know
/// stops the start.
pub fn configure_from_env() -> Result<Mode, String> {
    let mode = Mode::parse(std::env::var("MAILRS_CSP_MODE").ok().as_deref())?;
    set_mode(mode);
    Ok(mode)
}

/// Fix the mode; the first call wins. Tests that build a router in
/// enforce mode call it before building.
pub fn set_mode(mode: Mode) {
    if MODE.set(mode).is_err() && MODE.get() != Some(&mode) {
        panic!("the CSP mode was already set to {:?}", MODE.get());
    }
}

pub fn mode() -> Mode {
    MODE.get().copied().unwrap_or_default()
}

/// The header the policy goes out in.
pub fn header() -> HeaderName {
    mode().header()
}

/// The policy for a PDF attachment, which the reading pane shows in a
/// frame of its own page and nobody else's.
pub fn framed_by_self() -> String {
    POLICY.replace("frame-ancestors 'none'", "frame-ancestors 'self'")
}

/// Reports logged per minute at most; the rest are only counted.
const LOGGED_PER_MINUTE: u64 = 30;

static MINUTE: AtomicU64 = AtomicU64::new(0);
static IN_MINUTE: AtomicU64 = AtomicU64::new(0);

/// `POST /api/csp-report` — a browser's report of something the policy
/// blocked or would have blocked. Not signed in: a browser sends it on its
/// own, with no token to check. Only the directive and the blocked origin
/// are logged; page paths and mail content are not. Every report is
/// counted for the ops daemon (see [`super::csp_tally`]).
pub async fn receive(headers: HeaderMap, body: Bytes) -> StatusCode {
    let report: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    // `application/csp-report` wraps one report; the Reporting API sends a list
    let one = report
        .get("csp-report")
        .or_else(|| report.get(0).and_then(|r| r.get("body")));
    let Some(one) = one else {
        return StatusCode::NO_CONTENT;
    };
    let directive = field(
        one,
        &[
            "effective-directive",
            "effectiveDirective",
            "violated-directive",
        ],
    );
    let blocked = origin_of(&field(one, &["blocked-uri", "blockedURL"]));
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    super::csp_tally::record(
        super::csp_tally::Report::new(
            &directive,
            &field(one, &["violated-directive", "violatedDirective"]),
            &blocked,
            &field(one, &["document-uri", "documentURL"]),
            super::csp_tally::client_class(user_agent),
        ),
        now_secs(),
    );
    if take_log_slot(now_secs() as u64 / 60) {
        tracing::warn!(directive = %directive, blocked = %blocked, mode = mode().as_str(), "csp violation");
    }
    StatusCode::NO_CONTENT
}

fn field(v: &serde_json::Value, names: &[&str]) -> String {
    names
        .iter()
        .find_map(|n| v.get(*n).and_then(|x| x.as_str()))
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

/// Scheme and host of a URL, or the keyword a browser reports instead
/// (`inline`, `eval`, `data`).
fn origin_of(uri: &str) -> String {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return uri.to_string();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // a user name or password in the URL stays out of the log
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    format!("{scheme}://{host}")
}

fn now_secs() -> i64 {
    crate::handlers::complete::now_secs()
}

fn take_log_slot(minute: u64) -> bool {
    if MINUTE.swap(minute, Ordering::Relaxed) != minute {
        IN_MINUTE.store(0, Ordering::Relaxed);
    }
    IN_MINUTE.fetch_add(1, Ordering::Relaxed) < LOGGED_PER_MINUTE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_origin_of_a_blocked_url_is_kept() {
        assert_eq!(
            origin_of("https://cdn.example.com/a/b.js?token=x"),
            "https://cdn.example.com"
        );
        assert_eq!(origin_of("inline"), "inline");
        assert_eq!(
            origin_of("https://user:secret@cdn.example.com/x"),
            "https://cdn.example.com"
        );
    }

    #[test]
    fn the_mode_is_report_only_unless_enforce_is_asked_for() {
        assert_eq!(Mode::parse(None), Ok(Mode::ReportOnly));
        assert_eq!(Mode::parse(Some("report-only")), Ok(Mode::ReportOnly));
        assert_eq!(Mode::parse(Some(" enforce ")), Ok(Mode::Enforce));
        assert!(Mode::parse(Some("on")).is_err());
        assert_eq!(
            Mode::ReportOnly.header(),
            axum::http::header::CONTENT_SECURITY_POLICY_REPORT_ONLY
        );
        assert_eq!(
            Mode::Enforce.header(),
            axum::http::header::CONTENT_SECURITY_POLICY
        );
    }

    #[test]
    fn a_flood_of_reports_logs_a_few_a_minute() {
        let minute = 7;
        let logged = (0..100).filter(|_| take_log_slot(minute)).count() as u64;
        assert_eq!(logged, LOGGED_PER_MINUTE);
        assert!(take_log_slot(minute + 1), "a new minute logs again");
    }
}

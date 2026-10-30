//! The content security policy, sent report-only for now, and where
//! browsers send what it would have blocked.
//!
//! Mail is drawn into a shadow root of the page, so the page's policy is
//! the mail's policy too: styles inline in a message are allowed. Images
//! come only from this site — remote ones are the copies fastcore fetched
//! when the mail arrived. Scripts come only from this site.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Bytes;
use axum::http::StatusCode;

pub const POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; \
frame-src 'self' blob:; worker-src 'self'; manifest-src 'self'; object-src 'none'; \
base-uri 'none'; form-action 'self'; frame-ancestors 'none'; report-uri /api/csp-report";

/// The header the policy goes out in. Report-only until a week of real
/// traffic reports nothing; switching to enforcement is this one line.
pub const HEADER: axum::http::HeaderName = axum::http::header::CONTENT_SECURITY_POLICY_REPORT_ONLY;

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
/// would have blocked. Not signed in: a browser sends it on its own, with
/// no token to check. Only the directive and the blocked origin are
/// logged; page paths and mail content are not.
pub async fn receive(body: Bytes) -> StatusCode {
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
    if take_log_slot(now_minute()) {
        tracing::warn!(directive = %directive, blocked = %blocked, "csp violation (report-only)");
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

fn now_minute() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 60)
        .unwrap_or(0)
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
    fn a_flood_of_reports_logs_a_few_a_minute() {
        let minute = 7;
        let logged = (0..100).filter(|_| take_log_slot(minute)).count() as u64;
        assert_eq!(logged, LOGGED_PER_MINUTE);
        assert!(take_log_slot(minute + 1), "a new minute logs again");
    }
}

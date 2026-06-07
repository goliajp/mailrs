//! POST /api/web-errors — receives a structured error report from the
//! frontend's top-level ErrorBoundary and logs it via tracing so the
//! event shows up in journalctl / docker logs alongside every other
//! server-side error. No persistence — observability only.
//!
//! Intentionally unauthenticated. The user may not have a valid session
//! at the moment the error fires (logout state, login screen, expired
//! token). The general rate-limit middleware already shields the
//! endpoint from spam. Body is capped at MAX_REPORT_BYTES so a misbehaving
//! client can't flood us.

use axum::{extract::Json, http::StatusCode, response::IntoResponse};
use serde::Deserialize;

/// Per-report payload cap. A reasonable stack trace + metadata sits well
/// below 4 KB; anything larger is either malicious or a client bug.
const MAX_REPORT_BYTES: usize = 8 * 1024;

#[derive(Debug, Deserialize)]
pub struct WebErrorReport {
    pub build_version: Option<String>,
    pub error_message: String,
    pub error_name: Option<String>,
    pub error_stack: Option<String>,
    pub location_pathname: Option<String>,
    pub occurred_at: Option<String>,
    pub user_agent: Option<String>,
}

pub async fn submit(Json(report): Json<WebErrorReport>) -> impl IntoResponse {
    // Truncate egregiously-long fields rather than reject — better to log
    // something than nothing.
    let message = truncate(&report.error_message, 1024);
    let stack = report.error_stack.as_deref().map(|s| truncate(s, 4096));
    let user_agent = report.user_agent.as_deref().map(|s| truncate(s, 256));
    let pathname = report.location_pathname.as_deref().map(|s| truncate(s, 256));

    tracing::warn!(
        event = "web_error_report",
        error_name = report.error_name.as_deref().unwrap_or("Error"),
        error_message = %message,
        error_stack = stack.as_deref().unwrap_or(""),
        build_version = report.build_version.as_deref().unwrap_or("unknown"),
        location_pathname = pathname.as_deref().unwrap_or(""),
        occurred_at = report.occurred_at.as_deref().unwrap_or(""),
        user_agent = user_agent.as_deref().unwrap_or(""),
        "frontend ErrorBoundary caught an error",
    );

    StatusCode::NO_CONTENT
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // grapheme-naive truncation is fine for log output; we just want
        // to avoid pathologically long lines flooding journalctl
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

#[allow(dead_code)]
pub const MAX_BODY_BYTES: usize = MAX_REPORT_BYTES;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 100), "hello");
    }

    #[test]
    fn truncate_long_string_gets_ellipsis() {
        let s = "a".repeat(100);
        let out = truncate(&s, 10);
        assert_eq!(out, format!("{}…", "a".repeat(10)));
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        // 3-byte multibyte chars; truncating at byte 4 must back up to a
        // valid boundary to avoid splitting a codepoint
        let s = "日本語日本語";
        let out = truncate(s, 4);
        // any cut at a valid char boundary ≤ 4 + ellipsis is acceptable
        assert!(out.ends_with('…'));
        assert!(out.len() <= 4 + 3); // ≤ 4 bytes prefix + 3-byte ellipsis
    }
}

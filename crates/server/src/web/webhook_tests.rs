//! Tests for `webhook` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn valid_https_url() {
    assert!(is_valid_webhook_url("https://example.com/webhook"));
    assert!(is_valid_webhook_url("https://hooks.slack.com/services/xxx"));
}

#[test]
fn valid_localhost_http_url() {
    assert!(is_valid_webhook_url("http://localhost:8080/hook"));
    assert!(is_valid_webhook_url("http://127.0.0.1:3000/hook"));
}

#[test]
fn invalid_http_url_non_localhost() {
    assert!(!is_valid_webhook_url("http://example.com/webhook"));
    assert!(!is_valid_webhook_url("http://192.168.1.1/hook"));
}

#[test]
fn invalid_url_no_scheme() {
    assert!(!is_valid_webhook_url("example.com/webhook"));
    assert!(!is_valid_webhook_url("ftp://example.com/webhook"));
}

#[test]
fn url_too_long_is_rejected() {
    let long_url = format!("https://example.com/{}", "a".repeat(2040));
    assert!(!is_valid_webhook_url(&long_url));
}

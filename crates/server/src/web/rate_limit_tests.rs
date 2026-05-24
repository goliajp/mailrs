//! Tests for `rate_limit` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

fn ip_str(last: u8) -> String {
    format!("10.0.0.{last}")
}

#[tokio::test]
async fn web_rate_limiter_new_defaults() {
    let limiter = WebRateLimiter::new();
    assert!(limiter.auth.is_empty());
    assert!(limiter.general.is_empty());
}

#[tokio::test]
async fn auth_limiter_allows_10_then_rejects() {
    let limiter = WebRateLimiter::new();
    let ip = ip_str(1);
    for _ in 0..10 {
        assert!(limiter.auth.check(&ip).await);
    }
    assert!(!limiter.auth.check(&ip).await);
}

#[tokio::test]
async fn general_limiter_allows_300_then_rejects() {
    let limiter = WebRateLimiter::new();
    let ip = ip_str(2);
    for _ in 0..300 {
        assert!(limiter.general.check(&ip).await);
    }
    assert!(!limiter.general.check(&ip).await);
}

#[tokio::test]
async fn auth_and_general_are_independent() {
    let limiter = WebRateLimiter::new();
    let ip = ip_str(3);
    // exhaust auth
    for _ in 0..10 {
        limiter.auth.check(&ip).await;
    }
    assert!(!limiter.auth.check(&ip).await);
    // general still has tokens
    assert!(limiter.general.check(&ip).await);
}

#[tokio::test]
async fn cleanup_works() {
    let limiter = WebRateLimiter::new();
    let ip = ip_str(4);
    limiter.auth.check(&ip).await;
    limiter.general.check(&ip).await;
    assert_eq!(limiter.auth.len().await, 1);
    assert_eq!(limiter.general.len().await, 1);
    // cleanup with far-future timestamp drops everything
    limiter.cleanup(u64::MAX).await;
    assert_eq!(limiter.auth.len().await, 0);
    assert_eq!(limiter.general.len().await, 0);
}

#[test]
fn rate_limit_response_has_correct_status() {
    let resp = rate_limit_response(60);
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[test]
fn rate_limit_response_has_retry_after_header() {
    let resp = rate_limit_response(120);
    let retry = resp.headers().get("retry-after").unwrap();
    assert_eq!(retry.to_str().unwrap(), "120");
}

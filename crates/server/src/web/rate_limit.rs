use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::inbound::rate_limit::{RateLimiter, TokenBucketConfig};

/// web API rate limiter holding separate buckets for different tiers
pub struct WebRateLimiter {
    /// strict limiter for auth endpoints (10 req/min)
    pub auth: RateLimiter,
    /// relaxed limiter for general API endpoints (100 req/min)
    pub general: RateLimiter,
}

impl WebRateLimiter {
    pub fn new() -> Self {
        Self {
            auth: RateLimiter::new(TokenBucketConfig {
                capacity: 10,
                refill_rate: 10.0 / 60.0,
            }),
            general: RateLimiter::new(TokenBucketConfig {
                capacity: 100,
                refill_rate: 100.0 / 60.0,
            }),
        }
    }

    /// remove stale entries (call periodically from cleanup task)
    pub fn cleanup(&self, before: std::time::Instant) {
        self.auth.cleanup_stale(before);
        self.general.cleanup_stale(before);
    }
}

/// axum middleware for auth-tier rate limiting (stricter)
pub async fn auth_rate_limit(
    State(limiter): State<Arc<WebRateLimiter>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if !limiter.auth.check(addr.ip()) {
        return rate_limit_response(60);
    }
    next.run(request).await
}

/// axum middleware for general-tier rate limiting (relaxed)
pub async fn general_rate_limit(
    State(limiter): State<Arc<WebRateLimiter>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if !limiter.general.check(addr.ip()) {
        return rate_limit_response(60);
    }
    next.run(request).await
}

fn rate_limit_response(retry_after_secs: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(
            axum::http::header::RETRY_AFTER,
            retry_after_secs.to_string(),
        )],
        axum::Json(serde_json::json!({
            "error": "too many requests"
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_rate_limiter_new_defaults() {
        let limiter = WebRateLimiter::new();
        assert!(limiter.auth.is_empty());
        assert!(limiter.general.is_empty());
    }

    #[test]
    fn auth_limiter_allows_10_then_rejects() {
        let limiter = WebRateLimiter::new();
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 1));
        for _ in 0..10 {
            assert!(limiter.auth.check(ip));
        }
        assert!(!limiter.auth.check(ip));
    }

    #[test]
    fn general_limiter_allows_100_then_rejects() {
        let limiter = WebRateLimiter::new();
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 2));
        for _ in 0..100 {
            assert!(limiter.general.check(ip));
        }
        assert!(!limiter.general.check(ip));
    }

    #[test]
    fn auth_and_general_are_independent() {
        let limiter = WebRateLimiter::new();
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 3));
        // exhaust auth
        for _ in 0..10 {
            limiter.auth.check(ip);
        }
        assert!(!limiter.auth.check(ip));
        // general still has tokens
        assert!(limiter.general.check(ip));
    }

    #[test]
    fn cleanup_works() {
        let limiter = WebRateLimiter::new();
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 4));
        limiter.auth.check(ip);
        limiter.general.check(ip);
        assert_eq!(limiter.auth.len(), 1);
        assert_eq!(limiter.general.len(), 1);
        // cleanup with future timestamp
        limiter.cleanup(std::time::Instant::now() + std::time::Duration::from_secs(3600));
        assert_eq!(limiter.auth.len(), 0);
        assert_eq!(limiter.general.len(), 0);
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
}

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::inbound::rate_limit::{RateLimitStore, RateLimiter, TokenBucketConfig};

/// web API rate limiter holding separate buckets for different tiers
pub struct WebRateLimiter {
    /// strict limiter for auth endpoints (10 req/min)
    pub auth: RateLimiter,
    /// relaxed limiter for general API endpoints (300 req/min)
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
                capacity: 300,
                refill_rate: 300.0 / 60.0,
            }),
        }
    }

    /// remove stale entries (call periodically from cleanup task)
    pub async fn cleanup(&self, before_unix_secs: u64) {
        self.auth.cleanup_stale(before_unix_secs).await;
        self.general.cleanup_stale(before_unix_secs).await;
    }
}

/// axum middleware for auth-tier rate limiting (stricter)
pub async fn auth_rate_limit(
    State(limiter): State<Arc<WebRateLimiter>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if !limiter.auth.check(&addr.ip().to_string()).await {
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
    if !limiter.general.check(&addr.ip().to_string()).await {
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
#[path = "rate_limit_tests.rs"]
mod tests;

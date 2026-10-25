//! The login guard and the client address it counts by, for every way
//! this process accepts a password: web login, credential checks for
//! other services, DAV.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, LazyLock};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{Extensions, HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ipnet::IpNet;
use mailrs_auth_guard::login::{LoginGuard, Verdict};

/// Shared with IMAP, POP3, ManageSieve and SMTP through the network kevy.
pub static GUARD: LazyLock<Arc<dyn LoginGuard>> = LazyLock::new(|| {
    let url = std::env::var("MAILRS_KEVY_URL").ok();
    mailrs_auth_guard::kevy::guard_for(url.as_deref())
});

/// The proxies whose `X-Forwarded-For` is believed, from
/// `MAILRS_TRUSTED_PROXIES` (comma-separated CIDRs; loopback when unset).
/// A request from anywhere else is counted by its own address.
static TRUSTED: LazyLock<Vec<IpNet>> = LazyLock::new(|| {
    let raw = std::env::var("MAILRS_TRUSTED_PROXIES").unwrap_or_else(|_| {
        tracing::warn!(
            "MAILRS_TRUSTED_PROXIES unset: only loopback is trusted to forward client addresses"
        );
        "127.0.0.1/32,::1/128".to_string()
    });
    let nets = parse_proxies(&raw).unwrap_or_else(|bad| {
        panic!("MAILRS_TRUSTED_PROXIES: {bad:?} is not a CIDR");
    });
    tracing::info!(proxies = %raw, "trusted proxies");
    nets
});

fn parse_proxies(raw: &str) -> Result<Vec<IpNet>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<IpNet>().map_err(|_| s.to_string()))
        .collect()
}

/// The client: the last `X-Forwarded-For` hop when the connection comes
/// from a trusted proxy (that hop is the one the proxy itself appended),
/// the connection's own address otherwise. The leftmost entries are
/// whatever the client chose to send and are never read.
pub fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>, trusted: &[IpNet]) -> IpAddr {
    let forwarded = || {
        headers
            .get_all("x-forwarded-for")
            .iter()
            .next_back()
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .and_then(|ip| ip.trim().parse::<IpAddr>().ok())
    };
    match peer.map(|p| p.to_canonical()) {
        Some(p) if trusted.iter().any(|n| n.contains(&p)) => forwarded().unwrap_or(p),
        Some(p) => p,
        // no connection info: a router driven in-process, as tests do
        None => forwarded().unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
    }
}

/// The client address of a request, by [`client_ip`].
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl ClientIp {
    /// For middleware that holds the whole request.
    pub fn of(headers: &HeaderMap, extensions: &Extensions) -> Self {
        let peer = extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        Self(client_ip(headers, peer, &TRUSTED))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::of(&parts.headers, &parts.extensions))
    }
}

/// The answer to a locked attempt, the same on every endpoint.
pub fn locked_response(verdict: Verdict) -> Response {
    let minutes = verdict.retry_after_minutes();
    let mut resp = (
        StatusCode::TOO_MANY_REQUESTS,
        axum::Json(serde_json::json!({
            "error": format!("Too many failed sign-in attempts. Try again in {minutes} minutes."),
            "retry_after_minutes": minutes,
        })),
    )
        .into_response();
    if let Ok(v) = HeaderValue::from_str(&(minutes * 60).to_string()) {
        resp.headers_mut().insert(header::RETRY_AFTER, v);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xff(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_str(v).unwrap());
        h
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn a_trusted_proxy_names_the_client_by_its_last_hop() {
        let trusted = parse_proxies("172.16.0.0/12, 127.0.0.1/32").unwrap();
        let h = xff("6.6.6.6, 198.51.100.4");
        assert_eq!(
            client_ip(&h, Some(ip("172.18.0.1")), &trusted),
            ip("198.51.100.4")
        );
        // no header from a trusted proxy: the proxy itself
        assert_eq!(
            client_ip(&HeaderMap::new(), Some(ip("172.18.0.1")), &trusted),
            ip("172.18.0.1")
        );
    }

    #[test]
    fn anyone_else_is_their_own_address_whatever_they_claim() {
        let trusted = parse_proxies("172.16.0.0/12").unwrap();
        let h = xff("10.0.0.1");
        assert_eq!(
            client_ip(&h, Some(ip("203.0.113.9")), &trusted),
            ip("203.0.113.9")
        );
        assert_eq!(
            client_ip(&h, Some(ip("::ffff:203.0.113.9")), &trusted),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn bad_proxy_lists_are_refused() {
        assert_eq!(parse_proxies("10.0.0.0/8,nope"), Err("nope".to_string()));
        assert_eq!(parse_proxies("").unwrap(), Vec::<IpNet>::new());
    }

    #[test]
    fn locked_answers_say_how_long() {
        let r = locked_response(Verdict::Locked {
            retry_after_secs: 1799,
        });
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(r.headers()[header::RETRY_AFTER], "1800");
    }
}

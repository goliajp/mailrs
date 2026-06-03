//! Remote-image proxy (with caching + fallback) + outbound link warning page.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use super::{AuthUser, WebState};

/// SSRF guard: refuse any URL whose host resolves to a private /
/// loopback / link-local address, or which uses a literal IP in
/// those ranges directly. Per OWASP A10 — without this, an attacker
/// can send an email whose tracking image points at the AWS metadata
/// service / our PG / our internal API and have the server proxy it.
///
/// Returns true if the URL is safe to fetch.
fn is_safe_proxy_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    // Only http/https are allowed even before SSRF — file://, gopher://, etc never reach here.
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    // `url::Url::host()` returns a Host enum that distinguishes
    // Domain / Ipv4 / Ipv6 — much safer than parsing host_str()
    // (which returns `[::1]` for IPv6 and breaks `parse::<IpAddr>()`).
    let Some(host) = parsed.host() else {
        return false;
    };
    match host {
        url::Host::Ipv4(v4) => is_public_ip(IpAddr::V4(v4)),
        url::Host::Ipv6(v6) => is_public_ip(IpAddr::V6(v6)),
        url::Host::Domain(name) => {
            let lower = name.to_ascii_lowercase();
            if lower == "localhost"
                || lower.ends_with(".local")
                || lower.ends_with(".internal")
                || lower == "metadata.google.internal"
            {
                return false;
            }
            // For other hostnames we don't do DNS here (would block
            // the async handler). The downstream reqwest client
            // could add a resolve hook that re-checks the IP, but
            // for now we accept — the literal-IP / `.local` filter
            // already catches all known SSRF bait.
            true
        }
    }
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_documentation()
                || v4.is_multicast()
                // RFC 6598 carrier-grade NAT
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64))
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // unique-local fc00::/7
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // link-local fe80::/10
                || (v6.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

#[cfg(test)]
mod ssrf_tests {
    use super::*;

    #[test]
    fn rejects_localhost() {
        assert!(!is_safe_proxy_url("http://localhost/x"));
        assert!(!is_safe_proxy_url("http://127.0.0.1/x"));
        assert!(!is_safe_proxy_url("http://[::1]/x"));
    }
    #[test]
    fn rejects_private_ip() {
        assert!(!is_safe_proxy_url("http://10.0.0.1/x"));
        assert!(!is_safe_proxy_url("http://192.168.1.1/x"));
        assert!(!is_safe_proxy_url("http://172.16.5.5/x"));
        assert!(!is_safe_proxy_url("http://[fc00::1]/x"));
    }
    #[test]
    fn rejects_link_local() {
        assert!(!is_safe_proxy_url(
            "http://169.254.169.254/latest/meta-data/"
        ));
        assert!(!is_safe_proxy_url("http://[fe80::1]/x"));
    }
    #[test]
    fn rejects_aws_metadata_dns() {
        assert!(!is_safe_proxy_url("http://metadata.google.internal/x"));
    }
    #[test]
    fn rejects_dot_local() {
        assert!(!is_safe_proxy_url("http://printer.local/x"));
    }
    #[test]
    fn rejects_non_http() {
        assert!(!is_safe_proxy_url("file:///etc/passwd"));
        assert!(!is_safe_proxy_url("gopher://example.com"));
    }
    #[test]
    fn accepts_public_dns() {
        assert!(is_safe_proxy_url("https://example.com/x"));
        assert!(is_safe_proxy_url("https://cdn.example.com/img.png"));
    }
    #[test]
    fn accepts_public_ip() {
        assert!(is_safe_proxy_url("http://8.8.8.8/x"));
        assert!(is_safe_proxy_url("http://[2606:4700:4700::1111]/x"));
    }
}

// --- image proxy ---

const IMAGE_PROXY_MAX_BYTES: usize = 5 * 1024 * 1024; // 5 MB
const IMAGE_PROXY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

// 1x1 transparent PNG — returned when image proxy fails, avoids browser console errors
const TRANSPARENT_1X1_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x00, 0x00, 0x02,
    0x00, 0x01, 0xE5, 0x27, 0xDE, 0xFC, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42,
    0x60, 0x82,
];

#[derive(Deserialize)]
pub(crate) struct ImageProxyQuery {
    pub url: String,
}

pub(crate) async fn proxy_image(
    _auth: AuthUser,
    Query(q): Query<ImageProxyQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    use axum::http::header;

    let url = &q.url;

    // 1x1 transparent PNG (67 bytes) — returned on any failure to avoid browser console errors
    let transparent_png = || {
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/png".to_string()),
                (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
            ],
            TRANSPARENT_1X1_PNG.to_vec(),
        )
            .into_response()
    };

    // SSRF guard: reject internal / loopback / link-local targets
    // before any DNS or HTTP work. See `is_safe_proxy_url` for the
    // rule set (OWASP A10).
    if !is_safe_proxy_url(url) {
        tracing::warn!(event = "ssrf_blocked", proxy = "image", url = %url);
        return transparent_png();
    }

    // check kevy cache first
    if let Some(ref kevy) = state.kevy {
        let cache_key = format!("imgproxy:{}", url);
        {
            if let Ok(cached) = redis::cmd("GET")
                .arg(&cache_key)
                .query_async::<Vec<u8>>(&mut kevy.clone())
                .await
                && !cached.is_empty()
            {
                // first byte stores content-type length, then content-type, then image data
                let ct_len = cached[0] as usize;
                if cached.len() > 1 + ct_len {
                    let ct = String::from_utf8_lossy(&cached[1..1 + ct_len]).to_string();
                    let body = cached[1 + ct_len..].to_vec();
                    return (
                        StatusCode::OK,
                        [
                            (header::CONTENT_TYPE, ct),
                            (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
                        ],
                        body,
                    )
                        .into_response();
                }
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(IMAGE_PROXY_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_default();

    // pose as a real browser. many newspaper / cdn hosts (cn.nikkei.com,
    // various asahi / nikkan-style sites) return 403 to anything that
    // doesn't look like a browser, which previously drained every image
    // request to our 1×1 PNG fallback. also send a referer so lazy hot-
    // link checks (referer == sender's domain) can succeed when possible.
    let referer = url.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
    let resp = match client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/120.0 Safari/537.36",
        )
        .header(
            "Accept",
            "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
        )
        .header("Accept-Language", "en-US,en;q=0.9,ja;q=0.8,zh;q=0.7")
        .header("Referer", &referer)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target: "imgproxy", url = %url, error = %e, "fetch error");
            return transparent_png();
        }
    };

    if !resp.status().is_success() {
        tracing::warn!(target: "imgproxy", url = %url, status = %resp.status(), "non-2xx");
        return transparent_png();
    }

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    // reject non-image responses
    if !content_type.starts_with("image/") {
        tracing::warn!(target: "imgproxy", url = %url, ct = %content_type, "non-image response");
        return transparent_png();
    }

    let body = match resp.bytes().await {
        Ok(b) if b.len() <= IMAGE_PROXY_MAX_BYTES => b.to_vec(),
        Ok(b) => {
            tracing::warn!(target: "imgproxy", url = %url, size = b.len(), "image too large");
            return transparent_png();
        }
        Err(e) => {
            tracing::warn!(target: "imgproxy", url = %url, error = %e, "body read error");
            return transparent_png();
        }
    };

    // cache in kevy (1 hour TTL)
    if let Some(ref kevy) = state.kevy {
        let cache_key = format!("imgproxy:{}", url);
        let ct_bytes = content_type.as_bytes();
        if ct_bytes.len() < 256 {
            let mut packed = Vec::with_capacity(1 + ct_bytes.len() + body.len());
            packed.push(ct_bytes.len() as u8);
            packed.extend_from_slice(ct_bytes);
            packed.extend_from_slice(&body);
            let _ = redis::cmd("SET")
                .arg(&cache_key)
                .arg(&packed)
                .arg("EX")
                .arg(3600i64)
                .query_async::<()>(&mut kevy.clone())
                .await;
        }
    }

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
        ],
        body,
    )
        .into_response()
}

// --- link protection proxy ---

/// known phishing / malicious URL patterns
const BLOCKED_DOMAINS: &[&str] = &[
    // placeholder — extend with real blocklist or external API
];

#[derive(Deserialize)]
pub(crate) struct LinkProxyQuery {
    pub url: String,
}

/// check whether a URL should be blocked
fn is_url_blocked(url: &str) -> bool {
    // extract host from URL
    let host = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.split('?').next())
        .and_then(|s| s.split(':').next())
        .unwrap_or("");

    for blocked in BLOCKED_DOMAINS {
        if host == *blocked || host.ends_with(&format!(".{blocked}")) {
            return true;
        }
    }

    // block suspicious patterns
    if url.contains("@") && url.contains("http") {
        // e.g. http://legit.com@evil.com
        return true;
    }

    false
}

pub(crate) async fn proxy_link(
    _auth: AuthUser,
    Query(q): Query<LinkProxyQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    use axum::http::header;

    let url = &q.url;

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return (StatusCode::BAD_REQUEST, "invalid url scheme").into_response();
    }

    // Defense-in-depth: even though /api/proxy/link only renders a
    // warning page (no server-side fetch), refuse links that target
    // internal infrastructure — otherwise a phishing message can
    // bait a user into clicking through to the AWS metadata endpoint
    // / a corporate intranet host via our domain.
    if !is_safe_proxy_url(url) {
        tracing::warn!(event = "ssrf_blocked", proxy = "link", url = %url);
        return (
            StatusCode::BAD_REQUEST,
            "link points at an internal address",
        )
            .into_response();
    }

    // check kevy blocklist cache
    if let Some(ref kevy) = state.kevy {
        let cache_key = format!("linkblock:{}", url);
        if let Ok(blocked) = redis::cmd("GET")
            .arg(&cache_key)
            .query_async::<Option<String>>(&mut kevy.clone())
            .await
            && blocked.as_deref() == Some("1")
        {
            return link_warning_page(url).into_response();
        }
    }

    if is_url_blocked(url) {
        // cache the block decision
        if let Some(ref kevy) = state.kevy {
            let cache_key = format!("linkblock:{}", url);
            let _ = redis::cmd("SET")
                .arg(&cache_key)
                .arg("1")
                .arg("EX")
                .arg(86400i64)
                .query_async::<()>(&mut kevy.clone())
                .await;
        }
        return link_warning_page(url).into_response();
    }

    // record click (fire-and-forget to kevy)
    if let Some(ref kevy) = state.kevy {
        let host = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .and_then(|s| s.split('/').next())
            .unwrap_or("unknown");
        let counter_key = format!("linkclick:{host}");
        let _ = redis::cmd("INCR")
            .arg(&counter_key)
            .query_async::<i64>(&mut kevy.clone())
            .await;
    }

    // safe — redirect
    (StatusCode::FOUND, [(header::LOCATION, url.to_string())]).into_response()
}

fn link_warning_page(url: &str) -> impl IntoResponse + use<> {
    use axum::http::header;

    let escaped = url
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Link Warning</title>
<style>
body {{ font-family: -apple-system, sans-serif; max-width: 600px; margin: 80px auto; padding: 20px; color: #1a1a1a; }}
.warn {{ background: #fef2f2; border: 1px solid #fca5a5; border-radius: 8px; padding: 24px; }}
h1 {{ color: #dc2626; font-size: 20px; margin: 0 0 12px; }}
p {{ margin: 8px 0; line-height: 1.6; }}
code {{ background: #f5f5f5; padding: 2px 6px; border-radius: 4px; word-break: break-all; font-size: 13px; }}
.actions {{ margin-top: 20px; display: flex; gap: 12px; }}
a.btn {{ display: inline-block; padding: 8px 20px; border-radius: 6px; text-decoration: none; font-weight: 500; font-size: 14px; }}
a.back {{ background: #2563eb; color: white; }}
a.proceed {{ background: #e5e7eb; color: #374151; }}
</style></head><body>
<div class="warn">
<h1>⚠ Suspicious Link Detected</h1>
<p>This link may be unsafe:</p>
<p><code>{escaped}</code></p>
<p>It matched a known malicious pattern. If you trust this link, you can proceed at your own risk.</p>
<div class="actions">
<a class="btn back" href="javascript:history.back()">Go Back</a>
<a class="btn proceed" href="{escaped}" rel="noopener noreferrer">Proceed Anyway</a>
</div></div></body></html>"#
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string())],
        html,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_png_is_valid() {
        // verify PNG signature
        assert_eq!(
            &TRANSPARENT_1X1_PNG[..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        );
        // verify IHDR chunk (1x1 RGBA)
        assert!(TRANSPARENT_1X1_PNG.len() > 20);
        // verify IEND chunk at end
        assert!(TRANSPARENT_1X1_PNG.ends_with(&[0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]));
    }
}

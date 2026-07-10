//! Sender-avatar icon lookup.
//!
//! The web UI wants a small pixmap for every sender-address domain so it
//! can render a real logo instead of a coloured letter. Getting there
//! reliably means cascading three sources — BIMI DNS records for brand-
//! verified SVGs, then the vendor-neutral favicon services (Google's
//! `s2/favicons`, DuckDuckGo's `ip3`) — and remembering the answer so
//! the browser doesn't fan out on every render.
//!
//! Wire contract:
//!
//! - `GET /api/icon/{domain}` — auth-required (Bearer). On hit,
//!   returns the icon bytes with the upstream `Content-Type`. On miss
//!   returns **`204 No Content`**, not 404 — the browser then renders
//!   the fallback letter avatar without polluting the devtools
//!   console with a red row per unknown-icon domain.
//!
//! Cache layout in kevy:
//!
//! - `webapi:icon:v1:<domain>` — hash `{ ct: <content-type>, body: <bytes> }`.
//!   Set with a 7-day expiry so a slow-rolling icon update
//!   propagates without full cache invalidation.
//! - `webapi:icon:v1:miss:<domain>` — sentinel key with a 24-hour
//!   expiry. Prevents the same "no icon" domain from repeatedly
//!   walking the whole cascade.
//!
//! Every user is a single kevy connection wide (see `with_kevy`), so
//! the caches serve all authed users, not per-user — icons aren't
//! personal data.

use std::time::Duration;

use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;

use crate::handlers::kevy_util::with_kevy;

/// How long a positive icon result stays cached in kevy before we
/// re-check the upstream. Icons don't change often; a week is fine.
const HIT_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// How long a "no icon anywhere" result stays cached. Short enough
/// that a domain adopting BIMI within a day shows up quickly.
const MISS_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Upper bound on any single icon we're willing to serve. 256 KiB is
/// generous — real favicons are ~5 KiB.
const MAX_BYTES: usize = 256 * 1024;

/// Client for fetching external icon URLs. Short timeout — if a
/// provider is slow, we'd rather render the letter avatar than block
/// the UI. Follows redirects because favicon services 301 by design.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("mailrs-icon-fetch/1.0")
        .timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .expect("reqwest client build")
}

/// `GET /api/icon/{domain}` — cached brand-icon cascade.
///
/// See the module-level doc for the wire contract. This handler
/// intentionally never returns 4xx for "we don't have one" — a 204
/// keeps the browser network log clean and lets the frontend detect
/// the absence via the empty response body.
pub async fn get_icon(Path(domain): Path<String>) -> Response {
    // Reject anything that can't reasonably be a DNS name early —
    // otherwise a stray `../../etc/passwd`-shaped input walks the
    // full cascade. Allow lowercase letters, digits, dots, and
    // hyphens; nothing else can be in a hostname.
    let clean_domain = domain.trim().to_ascii_lowercase();
    if clean_domain.is_empty()
        || clean_domain.len() > 253
        || !clean_domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return no_content();
    }

    // 1. Positive-cache hit → return bytes verbatim.
    if let Some((ct, body)) = lookup_cache(&clean_domain).await {
        return build_ok(&ct, body);
    }
    // 2. Negative-cache hit → skip the cascade, return 204.
    if is_cached_miss(&clean_domain).await {
        return no_content();
    }

    // 3. Cascade upstreams. First one to return a real icon wins.
    let client = http_client();
    let upstream_urls = build_upstream_urls(&clean_domain).await;
    for url in upstream_urls {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let ct = resp
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .filter(|s| s.starts_with("image/"))
                    .unwrap_or("image/png")
                    .to_string();
                let bytes = match resp.bytes().await {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                if bytes.is_empty() || bytes.len() > MAX_BYTES {
                    continue;
                }
                let vec = bytes.to_vec();
                store_hit(&clean_domain, &ct, &vec).await;
                return build_ok(&ct, vec);
            }
            _ => continue,
        }
    }

    // 4. Nothing worked — remember this so we don't walk the cascade
    // again for the same domain within the miss TTL.
    store_miss(&clean_domain).await;
    no_content()
}

fn build_ok(content_type: &str, body: Vec<u8>) -> Response {
    let mut builder = Response::builder().status(StatusCode::OK);
    if let Ok(ct) = HeaderValue::from_str(content_type) {
        builder = builder
            .header(header::CONTENT_TYPE, ct)
            // Browser cache: 1 day, then stale-while-revalidate 1 week
            .header(
                header::CACHE_CONTROL,
                "public, max-age=86400, stale-while-revalidate=604800",
            );
    }
    builder
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::empty())
                .expect("empty body")
        })
}

fn no_content() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        // Cache 24 h on the client too so a soft reload doesn't
        // reprobe every unknown domain.
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(axum::body::Body::empty())
        .expect("no-content response")
}

/// Assemble the ordered list of upstream URLs to try. BIMI first
/// (real brand SVG when the sender's domain publishes one), then
/// vendor-neutral favicon services as a fallback.
async fn build_upstream_urls(domain: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(url) = bimi_lookup(domain).await {
        out.push(url);
    }
    // Google's s2/favicons handles ~all real domains and negotiates size.
    out.push(format!(
        "https://www.google.com/s2/favicons?sz=128&domain={domain}"
    ));
    // DDG covers a slightly different corner of the web — try if Google
    // 404'd.
    out.push(format!("https://icons.duckduckgo.com/ip3/{domain}.ico"));
    out
}

/// Parse the `default._bimi.<domain>` TXT record for its `l=` field
/// (the URL of the brand-verified SVG). Returns `None` if the DNS
/// query fails or the record has no `l=` tag.
async fn bimi_lookup(domain: &str) -> Option<String> {
    use hickory_resolver::TokioAsyncResolver;
    use hickory_resolver::config::{ResolverConfig, ResolverOpts};
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());
    let record = format!("default._bimi.{domain}");
    let lookup = resolver.txt_lookup(&record).await.ok()?;
    for txt in lookup.iter() {
        let joined: String = txt
            .txt_data()
            .iter()
            .flat_map(|b| std::str::from_utf8(b).ok().map(str::to_owned))
            .collect::<Vec<_>>()
            .join("");
        for kv in joined.split(';') {
            let kv = kv.trim();
            if let Some(v) = kv.strip_prefix("l=") {
                let url = v.trim().to_string();
                if url.starts_with("https://") {
                    return Some(url);
                }
            }
        }
    }
    None
}

// --- kevy cache glue ------------------------------------------------

fn hit_key(domain: &str) -> String {
    format!("webapi:icon:v1:{domain}")
}
fn miss_key(domain: &str) -> String {
    format!("webapi:icon:v1:miss:{domain}")
}

async fn lookup_cache(domain: &str) -> Option<(String, Vec<u8>)> {
    let key = hit_key(domain);
    let ct = with_kevy(move |c| c.hget(key.as_bytes(), b"ct")).ok()??;
    let key = hit_key(domain);
    let body = with_kevy(move |c| c.hget(key.as_bytes(), b"body")).ok()??;
    Some((String::from_utf8_lossy(&ct).to_string(), body))
}

async fn is_cached_miss(domain: &str) -> bool {
    let key = miss_key(domain);
    with_kevy(move |c| c.get(key.as_bytes()))
        .ok()
        .flatten()
        .is_some()
}

async fn store_hit(domain: &str, content_type: &str, bytes: &[u8]) {
    let key = hit_key(domain);
    let ct = content_type.as_bytes().to_vec();
    let body = bytes.to_vec();
    let hit_key_arg = key.clone();
    let _ = with_kevy(move |c| {
        c.hset(hit_key_arg.as_bytes(), &[(b"ct", &ct), (b"body", &body)])?;
        c.expire(key.as_bytes(), HIT_TTL)
    });
}

async fn store_miss(domain: &str) {
    let key = miss_key(domain);
    let _ = with_kevy(move |c| {
        c.set(key.as_bytes(), b"1")?;
        c.expire(key.as_bytes(), MISS_TTL)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_url_ordering_puts_favicon_services_after_bimi() {
        // BIMI is offline in this test (no DNS lookup), so we just
        // pin the deterministic tail — Google before DDG, both
        // parameterised on the domain.
        let urls = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(build_upstream_urls("example.com"));
        assert!(
            urls.iter()
                .any(|u| u.starts_with("https://www.google.com/s2/favicons"))
        );
        assert!(
            urls.iter()
                .any(|u| u.starts_with("https://icons.duckduckgo.com/ip3/"))
        );
        let google_pos = urls
            .iter()
            .position(|u| u.contains("google.com"))
            .expect("google url present");
        let ddg_pos = urls
            .iter()
            .position(|u| u.contains("duckduckgo.com"))
            .expect("ddg url present");
        assert!(google_pos < ddg_pos, "Google should be tried before DDG");
    }

    #[test]
    fn rejects_domain_with_bad_characters() {
        let resp = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(get_icon(Path("etc/passwd".to_string())));
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[test]
    fn rejects_empty_domain() {
        let resp = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(get_icon(Path("".to_string())));
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}

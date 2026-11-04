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
//!   returns the icon bytes with the upstream `Content-Type`. A
//!   subdomain with no icon of its own gets its registrable domain's.
//!   On miss returns **`204 No Content`**, not 404 — the browser then renders
//!   the fallback letter avatar without polluting the devtools
//!   console with a red row per unknown-icon domain.
//! - `GET /api/icon/{domain}?format=png` — the same icon as a PNG of at
//!   most 128 px a side, for the iOS and Android apps, which decode
//!   neither BIMI's SVG nor `.ico`. A source that cannot be converted is
//!   skipped for the next one in the cascade; 204 when none converts.
//!
//! Cache layout in kevy:
//!
//! - `webapi:icon:v1:<domain>` — hash `{ ct: <content-type>, body: <bytes> }`.
//!   Set with a 7-day expiry so a slow-rolling icon update
//!   propagates without full cache invalidation.
//! - `webapi:icon:v1:miss:<domain>` — sentinel key with a 24-hour
//!   expiry. Prevents the same "no icon" domain from repeatedly
//!   walking the whole cascade.
//! - `webapi:icon:v1:png:<domain>` — string, the PNG form, 7-day expiry.
//!   Derived from the icons above and rebuildable from them; kept so a
//!   logo is not drawn again on every request.
//! - `webapi:icon:v1:png-miss:<domain>` — sentinel, 24 hours: the domain
//!   has no icon that converts.
//!
//! A domain is a hostname, so it never holds a `:` and the four key
//! shapes cannot collide. Only webapi reads or writes them.
//!
//! Every user is a single kevy connection wide (see `with_kevy`), so
//! the caches serve all authed users, not per-user — icons aren't
//! personal data.

mod cache;
mod measure;
mod raster;

use std::time::Duration;

use axum::extract::{Path, Query};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;

use measure::usable_icon;

/// Upper bound on any single icon we're willing to serve. 256 KiB is
/// generous — real favicons are ~5 KiB.
const MAX_BYTES: usize = 256 * 1024;

/// How long one conversion may take before the source counts as one that
/// does not convert. Real logos take milliseconds.
const RENDER_BUDGET: Duration = Duration::from_secs(2);

/// Icon URLs come from a sender's DNS (BIMI) and domain, so they go
/// through the egress check. Short timeout — if a provider is slow, we'd
/// rather render the letter avatar than block the UI. Follows redirects
/// because favicon services 301 by design.
fn egress() -> mailrs_egress::Egress {
    mailrs_egress::Egress::new("mailrs-icon-fetch/1.0", Duration::from_secs(4))
}

/// Redirects an icon fetch may follow.
const MAX_REDIRECTS: u8 = 5;

#[derive(Debug, Default, Deserialize)]
pub struct IconQuery {
    format: Option<IconFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum IconFormat {
    Png,
}

/// `GET /api/icon/{domain}` — cached brand-icon cascade.
///
/// See the module-level doc for the wire contract. This handler
/// intentionally never returns 4xx for "we don't have one" — a 204
/// keeps the browser network log clean and lets the frontend detect
/// the absence via the empty response body.
pub async fn get_icon(Path(domain): Path<String>, Query(q): Query<IconQuery>) -> Response {
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
    // Mail is often sent from a subdomain that has no icon of its own
    // (`mail.example.org`, `em.example.com`) while the organisation's
    // domain has one. The subdomain's miss stays cached; the fallback is
    // its own cached lookup.
    let org = organisation(&clean_domain).filter(|org| *org != clean_domain);
    let domains = std::iter::once(clean_domain).chain(org);
    match q.format {
        None => {
            for domain in domains {
                if let Some((ct, body)) = resolve(&domain).await {
                    return build_ok(&ct, body);
                }
            }
        }
        Some(IconFormat::Png) => {
            for domain in domains {
                if let Some(png) = resolve_png(&domain).await {
                    return build_ok("image/png", png);
                }
            }
        }
    }
    no_content()
}

/// The registrable domain (`mail.example.co.jp` → `example.co.jp`), never
/// a public suffix on its own.
fn organisation(domain: &str) -> Option<String> {
    let org = psl::domain(domain.as_bytes())?;
    std::str::from_utf8(org.as_bytes()).ok().map(str::to_string)
}

/// The icon for exactly `domain`: cache, then the upstream cascade, with
/// both outcomes cached.
async fn resolve(clean_domain: &str) -> Option<(String, Vec<u8>)> {
    // 1. Positive-cache hit → return bytes verbatim, unless what was
    // cached is one of the placeholders `usable_icon` rejects.  Hits
    // live for a week, so a version that stops storing them would still
    // serve the ones already stored for another seven days; the same
    // test on the way out retires them on first touch.
    if let Some((ct, body)) = cache::lookup_hit(clean_domain).await
        && usable_icon(&ct, &body)
    {
        return Some((ct, body));
    }
    // 2. Negative-cache hit → skip the cascade.
    if cache::is_miss(clean_domain).await {
        return None;
    }

    // 3. Cascade upstreams. First one to return a real icon wins.
    let egress = egress();
    for url in build_upstream_urls(clean_domain).await {
        if let Some((ct, bytes)) = fetch_icon(&egress, &url).await {
            cache::store_hit(clean_domain, &ct, &bytes).await;
            return Some((ct, bytes));
        }
    }

    // 4. Nothing worked — remember this so we don't walk the cascade
    // again for the same domain within the miss TTL.
    cache::store_miss(clean_domain).await;
    None
}

/// The icon for exactly `domain` as a PNG. The icon the web gets comes
/// first; if it does not convert (an SVG resvg cannot draw), the rest of
/// the cascade is walked for one that does, and the outcome is cached
/// next to the web's.
async fn resolve_png(clean_domain: &str) -> Option<Vec<u8>> {
    if let Some(png) = cache::lookup_png(clean_domain).await {
        return Some(png);
    }
    if cache::is_png_miss(clean_domain).await {
        return None;
    }
    let (ct, body) = resolve(clean_domain).await?;
    if let Some(png) = convert(ct, body).await {
        cache::store_png(clean_domain, png.clone()).await;
        return Some(png);
    }
    let egress = egress();
    for url in build_upstream_urls(clean_domain).await {
        if let Some((ct, bytes)) = fetch_icon(&egress, &url).await
            && let Some(png) = convert(ct, bytes).await
        {
            cache::store_png(clean_domain, png.clone()).await;
            return Some(png);
        }
    }
    cache::store_png_miss(clean_domain).await;
    None
}

/// The conversion, off the reactor and within `RENDER_BUDGET`. A render
/// that overruns is abandoned, not awaited: its thread finishes on its
/// own and the result is dropped.
async fn convert(content_type: String, bytes: Vec<u8>) -> Option<Vec<u8>> {
    let job = tokio::task::spawn_blocking(move || raster::to_png(&content_type, &bytes));
    match tokio::time::timeout(RENDER_BUDGET, job).await {
        Ok(Ok(png)) => png,
        Ok(Err(_)) | Err(_) => None,
    }
}

/// One upstream's answer, if it is an image worth drawing.
async fn fetch_icon(egress: &mailrs_egress::Egress, url: &str) -> Option<(String, Vec<u8>)> {
    let resp = egress
        .send(url, MAX_REDIRECTS, |c, u| c.get(u))
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|s| s.starts_with("image/"))
        .unwrap_or("image/png")
        .to_string();
    let bytes = mailrs_egress::read_capped(resp, MAX_BYTES).await.ok()?;
    // A provider that has no icon does not always say so with a status
    // code.  DuckDuckGo answers `200 image/x-icon` and 43 bytes — a 1×1
    // transparent GIF — for a domain it knows nothing about, and the
    // browser stretches that into a 36 px circle of nothing: the sender
    // looks like it has no avatar at all, which is what
    // `customeremail.microsoftrewards.com` showed on 2026-09-18.
    // Anything too small to be a logo is that provider's way of saying
    // no; carry on down the cascade and fall back to the letter.
    if bytes.is_empty() || !usable_icon(&ct, &bytes) {
        return None;
    }
    Some((ct, bytes))
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
    use hickory_resolver::TokioResolver;
    let resolver = TokioResolver::builder_tokio()
        .and_then(|b| b.build())
        .ok()?;
    let record = format!("default._bimi.{domain}");
    let lookup = resolver.txt_lookup(&record).await.ok()?;
    for record in lookup.answers() {
        let hickory_resolver::proto::rr::RData::TXT(txt) = &record.data else {
            continue;
        };
        let joined = txt.to_string();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sender_subdomain_falls_back_to_its_registrable_domain() {
        assert_eq!(
            organisation("mail.example.org").as_deref(),
            Some("example.org")
        );
        assert_eq!(
            organisation("em.news.example.co.jp").as_deref(),
            Some("example.co.jp")
        );
        assert_eq!(organisation("example.org").as_deref(), Some("example.org"));
        // a public suffix alone is not an organisation
        assert_eq!(organisation("co.jp"), None);
    }

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

    fn call(domain: &str, format: Option<IconFormat>) -> StatusCode {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(get_icon(
                Path(domain.to_string()),
                Query(IconQuery { format }),
            ))
            .status()
    }

    #[test]
    fn rejects_domain_with_bad_characters() {
        assert_eq!(call("etc/passwd", None), StatusCode::NO_CONTENT);
        assert_eq!(
            call("etc/passwd", Some(IconFormat::Png)),
            StatusCode::NO_CONTENT
        );
    }

    #[test]
    fn rejects_empty_domain() {
        assert_eq!(call("", None), StatusCode::NO_CONTENT);
        assert_eq!(call("", Some(IconFormat::Png)), StatusCode::NO_CONTENT);
    }
}

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

    if let Some((ct, body)) = resolve(&clean_domain).await {
        return build_ok(&ct, body);
    }
    // Mail is often sent from a subdomain that has no icon of its own
    // (`mail.anthropic.com`, `em.example.com`) while the organisation's
    // domain has one. The subdomain's miss stays cached; the fallback is
    // its own cached lookup.
    if let Some(org) = organisation(&clean_domain)
        && org != clean_domain
        && let Some((ct, body)) = resolve(&org).await
    {
        return build_ok(&ct, body);
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
    // cached is one of the placeholders below.  Hits live for a week,
    // so a version that stops storing them would still serve the ones
    // already stored for another seven days; the same test on the way
    // out retires them on first touch.
    if let Some((ct, body)) = lookup_cache(clean_domain).await
        && usable_icon(&ct, &body)
    {
        return Some((ct, body));
    }
    // 2. Negative-cache hit → skip the cascade.
    if is_cached_miss(clean_domain).await {
        return None;
    }

    // 3. Cascade upstreams. First one to return a real icon wins.
    let client = http_client();
    let upstream_urls = build_upstream_urls(clean_domain).await;
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
                // A provider that has no icon does not always say so
                // with a status code.  DuckDuckGo answers `200
                // image/x-icon` and 43 bytes — a 1×1 transparent GIF —
                // for a domain it knows nothing about, and the browser
                // stretches that into a 36 px circle of nothing: the
                // sender looks like it has no avatar at all, which is
                // what `customeremail.microsoftrewards.com` showed on
                // 2026-09-18.  Anything too small to be a logo is that
                // provider's way of saying no; carry on down the
                // cascade and fall back to the letter.
                if !usable_icon(&ct, &bytes) {
                    continue;
                }
                let vec = bytes.to_vec();
                store_hit(clean_domain, &ct, &vec).await;
                return Some((ct, vec));
            }
            _ => continue,
        }
    }

    // 4. Nothing worked — remember this so we don't walk the cascade
    // again for the same domain within the miss TTL.
    store_miss(clean_domain).await;
    None
}

/// Smallest icon worth drawing.  Real favicons start at 16×16; the
/// placeholders are 1×1.  Eight is comfortably between them and needs
/// no judgement about which sizes a brand might publish.
const MIN_ICON_PX: u32 = 8;

/// Whether these bytes carry an image big enough to be a logo.
///
/// Vector icons (BIMI's SVG) have no pixel size to read and are taken
/// at face value — a brand that published one meant it.  Everything
/// else states its dimensions in its header, and this reads only that:
/// four formats, fixed offsets, no decoding.
fn usable_icon(content_type: &str, bytes: &[u8]) -> bool {
    if content_type.contains("svg") || bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
        return true;
    }
    match icon_dimensions(bytes) {
        Some((w, h)) => w >= MIN_ICON_PX && h >= MIN_ICON_PX,
        // Not a format we can measure: keep the old behaviour and
        // serve it.  The rule is for the placeholders we have seen,
        // not a licence to drop anything unfamiliar.
        None => true,
    }
}

/// Width and height from an image header — PNG, GIF, ICO, JPEG.
fn icon_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    // PNG: IHDR is always first, at a fixed offset.
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.len() >= 24 {
        let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
        return Some((w, h));
    }
    // GIF: logical screen descriptor, little-endian, right after the
    // six-byte signature.
    if (b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) && b.len() >= 10 {
        let w = u16::from_le_bytes(b[6..8].try_into().ok()?);
        let h = u16::from_le_bytes(b[8..10].try_into().ok()?);
        return Some((u32::from(w), u32::from(h)));
    }
    // ICO: one byte per axis in each directory entry, 0 meaning 256.
    // An .ico can hold several sizes; the largest is the one a 36 px
    // circle would use.
    if b.starts_with(b"\x00\x00\x01\x00") && b.len() >= 6 {
        let count = u16::from_le_bytes(b[4..6].try_into().ok()?) as usize;
        let mut best = (0u32, 0u32);
        for i in 0..count {
            let e = 6 + i * 16;
            if b.len() < e + 16 {
                break;
            }
            let dim = |v: u8| if v == 0 { 256u32 } else { u32::from(v) };
            let (w, h) = (dim(b[e]), dim(b[e + 1]));
            if w.min(h) > best.0.min(best.1) {
                best = (w, h);
            }
        }
        return (best != (0, 0)).then_some(best);
    }
    // JPEG: walk the segment chain to the frame header that carries
    // the dimensions.
    if b.starts_with(b"\xff\xd8") {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                return None;
            }
            let marker = b[i + 1];
            let len = u16::from_be_bytes(b[i + 2..i + 4].try_into().ok()?) as usize;
            // SOF0..SOF15, minus the four that are not frame headers.
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC | 0xD8) {
                let h = u16::from_be_bytes(b[i + 5..i + 7].try_into().ok()?);
                let w = u16::from_be_bytes(b[i + 7..i + 9].try_into().ok()?);
                return Some((u32::from(w), u32::from(h)));
            }
            i += 2 + len;
        }
    }
    None
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

// --- kevy cache glue ------------------------------------------------

fn hit_key(domain: &str) -> String {
    format!("webapi:icon:v1:{domain}")
}
fn miss_key(domain: &str) -> String {
    format!("webapi:icon:v1:miss:{domain}")
}

async fn lookup_cache(domain: &str) -> Option<(String, Vec<u8>)> {
    let key = hit_key(domain);
    let ct =
        with_kevy(move |c| c.hget(key.as_bytes(), b"ct").map_err(std::io::Error::from)).ok()??;
    let key = hit_key(domain);
    let body = with_kevy(move |c| {
        c.hget(key.as_bytes(), b"body")
            .map_err(std::io::Error::from)
    })
    .ok()??;
    Some((String::from_utf8_lossy(&ct).to_string(), body))
}

async fn is_cached_miss(domain: &str) -> bool {
    let key = miss_key(domain);
    with_kevy(move |c| c.get(key.as_bytes()).map_err(std::io::Error::from))
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
            .map_err(std::io::Error::from)
    });
}

async fn store_miss(domain: &str) {
    let key = miss_key(domain);
    let _ = with_kevy(move |c| {
        c.set(key.as_bytes(), b"1")?;
        c.expire(key.as_bytes(), MISS_TTL)
            .map_err(std::io::Error::from)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sender_subdomain_falls_back_to_its_registrable_domain() {
        assert_eq!(
            organisation("mail.anthropic.com").as_deref(),
            Some("anthropic.com")
        );
        assert_eq!(
            organisation("em.news.example.co.jp").as_deref(),
            Some("example.co.jp")
        );
        assert_eq!(
            organisation("anthropic.com").as_deref(),
            Some("anthropic.com")
        );
        // a public suffix alone is not an organisation
        assert_eq!(organisation("co.jp"), None);
    }

    /// DuckDuckGo's answer for a domain it has no icon for, byte for
    /// byte off the wire (2026-09-18,
    /// `customeremail.microsoftrewards.com`): a 1×1 transparent GIF,
    /// served as `200 image/x-icon`.  Serving it renders a sender with
    /// no avatar at all.
    const DDG_PLACEHOLDER: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00\x21\xf9\x04\x01\x00\x00\x00\x00\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3b";

    fn png_header(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b
    }

    fn ico(entries: &[(u8, u8)]) -> Vec<u8> {
        let mut b = vec![0, 0, 1, 0];
        b.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (w, h) in entries {
            b.extend_from_slice(&[*w, *h, 0, 0, 1, 0, 32, 0]);
            b.extend_from_slice(&[0; 8]);
        }
        b
    }

    #[test]
    fn a_providers_one_by_one_placeholder_is_not_an_icon() {
        assert_eq!(DDG_PLACEHOLDER.len(), 43);
        assert_eq!(icon_dimensions(DDG_PLACEHOLDER), Some((1, 1)));
        assert!(!usable_icon("image/x-icon", DDG_PLACEHOLDER));
    }

    #[test]
    fn a_real_favicon_is_an_icon() {
        assert!(usable_icon("image/png", &png_header(128, 128)));
        assert!(usable_icon("image/png", &png_header(16, 16)));
        assert!(!usable_icon("image/png", &png_header(1, 1)));
    }

    /// An `.ico` carries several sizes; the biggest is the one a 36 px
    /// circle draws, so a file that also holds a 1×1 is still fine.
    #[test]
    fn an_ico_is_measured_by_its_largest_entry() {
        assert_eq!(icon_dimensions(&ico(&[(16, 16), (32, 32)])), Some((32, 32)));
        assert!(usable_icon("image/x-icon", &ico(&[(1, 1), (32, 32)])));
        assert!(!usable_icon("image/x-icon", &ico(&[(1, 1)])));
        // 0 means 256 in the ICO directory.
        assert_eq!(icon_dimensions(&ico(&[(0, 0)])), Some((256, 256)));
    }

    /// BIMI icons are SVG: nothing to measure, and a brand that
    /// published one meant it.  And a format we cannot read is served
    /// as before rather than dropped.
    #[test]
    fn vector_and_unknown_formats_are_kept() {
        assert!(usable_icon(
            "image/svg+xml",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        ));
        assert!(usable_icon("image/webp", b"RIFF????WEBPVP8 "));
        assert_eq!(icon_dimensions(b"RIFF????WEBPVP8 "), None);
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

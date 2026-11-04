//! The icon cache in network kevy. Key layout is documented on the
//! parent module; every key is spelled here and nowhere else.

use std::time::Duration;

use crate::handlers::kevy_util::with_kevy;

/// How long a positive icon result stays cached in kevy before we
/// re-check the upstream. Icons don't change often; a week is fine.
const HIT_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// How long a "no icon anywhere" result stays cached. Short enough
/// that a domain adopting BIMI within a day shows up quickly.
const MISS_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn hit_key(domain: &str) -> String {
    format!("webapi:icon:v1:{domain}")
}
fn miss_key(domain: &str) -> String {
    format!("webapi:icon:v1:miss:{domain}")
}
fn png_key(domain: &str) -> String {
    format!("webapi:icon:v1:png:{domain}")
}
fn png_miss_key(domain: &str) -> String {
    format!("webapi:icon:v1:png-miss:{domain}")
}

pub(super) async fn lookup_hit(domain: &str) -> Option<(String, Vec<u8>)> {
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

pub(super) async fn is_miss(domain: &str) -> bool {
    exists(miss_key(domain))
}

pub(super) async fn store_hit(domain: &str, content_type: &str, bytes: &[u8]) {
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

pub(super) async fn store_miss(domain: &str) {
    let key = miss_key(domain);
    let _ = with_kevy(move |c| {
        c.set(key.as_bytes(), b"1")?;
        c.expire(key.as_bytes(), MISS_TTL)
            .map_err(std::io::Error::from)
    });
}

pub(super) async fn lookup_png(domain: &str) -> Option<Vec<u8>> {
    let key = png_key(domain);
    with_kevy(move |c| c.get(key.as_bytes()).map_err(std::io::Error::from))
        .ok()
        .flatten()
}

pub(super) async fn is_png_miss(domain: &str) -> bool {
    exists(png_miss_key(domain))
}

pub(super) async fn store_png(domain: &str, png: Vec<u8>) {
    let key = png_key(domain);
    let _ = with_kevy(move |c| {
        c.set_with_ttl(key.as_bytes(), &png, HIT_TTL)
            .map_err(std::io::Error::from)
    });
}

pub(super) async fn store_png_miss(domain: &str) {
    let key = png_miss_key(domain);
    let _ = with_kevy(move |c| {
        c.set_with_ttl(key.as_bytes(), b"1", MISS_TTL)
            .map_err(std::io::Error::from)
    });
}

fn exists(key: String) -> bool {
    with_kevy(move |c| c.get(key.as_bytes()).map_err(std::io::Error::from))
        .ok()
        .flatten()
        .is_some()
}

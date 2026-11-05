//! Pasted images, stored in the network kevy until the message showing
//! them is sent:
//!
//! ```text
//! inline:<id>    hash  { content_type, body, owner }, expires after KEEP
//! ```
//!
//! A draft names an image by its URL, `/api/mail/inline/<id>`. The image
//! is kept while a draft names it: saving the draft and the upload sweep
//! both push its expiry out again once half of [`KEEP`] has passed.

use std::time::Duration;

use kevy_client::Connection;

/// How long an image nobody's draft names is kept.
pub const KEEP: Duration = Duration::from_secs(30 * 86_400);

/// The URL path an image is served under; its id follows.
pub const URL_PATH: &str = "/api/mail/inline/";

pub fn key(id: &str) -> String {
    format!("inline:{id}")
}

/// Whether `id` has the shape of an image id: 32 hex digits.
pub fn is_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The image ids `text` names by URL, in order, each once.
pub fn ids_in(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for (at, _) in text.match_indices(URL_PATH) {
        let start = at + URL_PATH.len();
        let Some(id) = text.get(start..start + 32) else {
            continue;
        };
        if is_id(id) && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Push the image's expiry back to [`KEEP`] when less than half of it is
/// left. `true` when it did; an image already gone stays gone.
pub fn keep(conn: &mut Connection, id: &str) -> std::io::Result<bool> {
    let key = key(id);
    let left = conn.ttl_ms(key.as_bytes())?;
    // -2: no such key; -1: no expiry, which nothing here sets
    if left < 0 || left as u128 >= KEEP.as_millis() / 2 {
        return Ok(false);
    }
    Ok(conn.expire(key.as_bytes(), KEEP)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn a_draft_names_its_images_by_url() {
        let body = format!(
            "<img src=\"/api/mail/inline/{A}\"><img src='https://m.test/api/mail/inline/{A}'>\
             <img src=\"/api/mail/inline/short\">"
        );
        assert_eq!(ids_in(&body), vec![A]);
        assert!(ids_in("no images").is_empty());
    }

    #[test]
    fn an_image_is_kept_once_half_its_time_has_gone() {
        let mut c = Connection::connect("mem://inline-keep").unwrap();
        let k = key(A);
        c.hset(k.as_bytes(), &[(b"body".as_slice(), b"x".as_slice())])
            .unwrap();
        c.expire(k.as_bytes(), KEEP).unwrap();
        assert!(!keep(&mut c, A).unwrap(), "still fresh: nothing written");
        c.expire(k.as_bytes(), Duration::from_secs(86_400)).unwrap();
        assert!(keep(&mut c, A).unwrap());
        assert!(c.ttl_ms(k.as_bytes()).unwrap() as u128 > KEEP.as_millis() / 2);
        assert!(!keep(&mut c, &"f".repeat(32)).unwrap(), "gone stays gone");
    }
}

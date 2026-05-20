//! Framework-agnostic HTTP response representation plus the small XML helpers
//! that DAV handlers compose multistatus / propstat bodies from.

use sha2::{Digest, Sha256};

/// A minimal HTTP response shape. Handlers return this; server-side wrapper
/// code (axum / actix / hyper) translates it into the framework's own response
/// type.
#[derive(Debug, Clone)]
pub struct DavResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl DavResponse {
    /// Empty response with `status` and no headers / body.
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Builder: append a header. Both `name` and `value` are stored verbatim;
    /// header-name canonicalisation is the server-side wrapper's job.
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Builder: set the body.
    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }
}

/// Short, stable ETag derived from a piece of content. Implemented as the
/// first 8 bytes of SHA-256 hex-encoded (16 ASCII chars), matching the
/// mailrs reference implementation.
pub fn etag_of(data: &str) -> String {
    let hash = Sha256::digest(data.as_bytes());
    hex::encode(&hash[..8])
}

/// XML-escape a text-content string for inclusion in an XML body.
///
/// Covers the five entities required by XML 1.0 plus the double quote
/// (needed inside attribute values that some clients emit).
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Build a `207 Multi-Status` response with the DAV namespaces declared and
/// `inner` wrapped inside `<D:multistatus>...</D:multistatus>`.
pub fn multistatus(inner: &str) -> DavResponse {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <D:multistatus xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\" \
         xmlns:CR=\"urn:ietf:params:xml:ns:carddav\" \
         xmlns:CS=\"http://calendarserver.org/ns/\">\n\
         {inner}\n\
         </D:multistatus>"
    );
    DavResponse::new(207)
        .with_header("content-type", "application/xml; charset=utf-8")
        .with_header("dav", "1, 2, 3, calendar-access, addressbook")
        .with_body(body.into_bytes())
}

/// Canonical OPTIONS response advertising CalDAV + CardDAV class support and
/// the verbs handlers in this crate understand.
pub fn options_response() -> DavResponse {
    DavResponse::new(200)
        .with_header("dav", "1, 2, 3, calendar-access, addressbook")
        .with_header(
            "allow",
            "OPTIONS, GET, PUT, DELETE, PROPFIND, REPORT, MKCALENDAR",
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_is_deterministic() {
        let a = etag_of("hello world");
        let b = etag_of("hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn etag_is_16_hex_chars() {
        let etag = etag_of("anything");
        assert_eq!(etag.len(), 16);
        assert!(etag.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_content_yields_different_etag() {
        assert_ne!(etag_of("a"), etag_of("b"));
    }

    #[test]
    fn xml_escape_covers_required_entities() {
        assert_eq!(xml_escape("a<b>c&d\"e"), "a&lt;b&gt;c&amp;d&quot;e");
    }

    #[test]
    fn xml_escape_passes_plain_text_through() {
        assert_eq!(xml_escape("just text"), "just text");
    }

    #[test]
    fn multistatus_wraps_body_with_namespaces() {
        let resp = multistatus("<D:response/>");
        assert_eq!(resp.status, 207);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("xmlns:D=\"DAV:\""));
        assert!(body.contains("xmlns:C=\"urn:ietf:params:xml:ns:caldav\""));
        assert!(body.contains("<D:response/>"));
    }

    #[test]
    fn options_response_advertises_dav_classes() {
        let resp = options_response();
        assert_eq!(resp.status, 200);
        let dav = resp
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("dav"))
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert!(dav.contains("calendar-access"));
        assert!(dav.contains("addressbook"));
    }

    #[test]
    fn dav_response_builder_appends_headers() {
        let resp = DavResponse::new(200)
            .with_header("content-type", "text/calendar")
            .with_header("etag", "\"abc\"");
        assert_eq!(resp.headers.len(), 2);
        assert_eq!(resp.headers[0].0, "content-type");
        assert_eq!(resp.headers[1].1, "\"abc\"");
    }
}

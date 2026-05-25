//! MIME tree parser: splits a message into a recursive [`Part`]
//! structure with parsed Content-Type, Content-Transfer-Encoding,
//! and decoded body bytes.

use mailrs_rfc5322::Message;

use crate::content_type::{ContentType, Disposition};
use crate::decoder::TransferEncoding;

/// One MIME part. A message is recursively a Part: the top-level
/// message is the root part, multipart bodies have child parts.
#[derive(Debug, Clone)]
pub struct Part {
    /// Parsed Content-Type. Defaults to `text/plain; charset=us-ascii`
    /// when the header is missing (RFC 2045 §5.2).
    pub content_type: ContentType,
    /// Parsed Content-Disposition, if present.
    pub disposition: Option<Disposition>,
    /// Content-ID header value, if present (used for cid: references
    /// in HTML).
    pub content_id: Option<String>,
    /// Content-Transfer-Encoding. Defaults to `7bit` (RFC 2045 §6.1).
    pub transfer_encoding: TransferEncoding,
    /// **Decoded** body bytes for leaf parts. For multipart parts,
    /// this is the inter-boundary preamble (usually empty / "this
    /// is a multipart message"-style text) and isn't typically
    /// useful — use `children` instead.
    pub body: Vec<u8>,
    /// Child parts for `multipart/*` types. Empty for leaf parts.
    pub children: Vec<Part>,
}

impl Part {
    /// Find the first descendant part (depth-first) matching
    /// `<type>/<subtype>` (lowercased compare). Returns `None` if
    /// no part matches.
    pub fn find_by_content_type(&self, mime_type: &str) -> Option<&Part> {
        // Split the target once into (type, subtype) and compare bytewise.
        // Recursive descent — no Vec allocation for the walk stack, which
        // [`Self::walk`] would otherwise do.
        let (target_type, target_subtype) = mime_type.split_once('/')?;
        find_by_ct_recursive(self, target_type, target_subtype)
    }

    /// Depth-first iterator over self + all descendant parts.
    pub fn walk(&self) -> Walker<'_> {
        Walker { stack: vec![self] }
    }

    /// Decode the body as text using the part's `charset=` parameter.
    /// Returns `None` if the part isn't `text/*`.
    pub fn body_text(&self) -> Option<String> {
        if self.content_type.type_ != "text" {
            return None;
        }
        let charset = self.content_type.charset();
        if let Some(enc) = encoding_rs::Encoding::for_label(charset.as_bytes()) {
            let (cow, _, _) = enc.decode(&self.body);
            Some(cow.into_owned())
        } else {
            // Unknown charset → lossy UTF-8.
            Some(String::from_utf8_lossy(&self.body).into_owned())
        }
    }

    /// Iterate over leaf parts marked as attachments. A part is an
    /// "attachment" when its `Content-Disposition: attachment` OR
    /// it has a `filename` parameter in either Content-Type or
    /// Content-Disposition. (Some mailers omit the disposition
    /// header but still set a filename.)
    pub fn attachments(&self) -> impl Iterator<Item = &Part> {
        self.walk().filter(|p| p.is_attachment())
    }

    /// Returns the attachment's filename if available — checks
    /// Content-Disposition `filename=` first, falls back to
    /// Content-Type `name=`.
    pub fn attachment_filename(&self) -> Option<&str> {
        if let Some(d) = &self.disposition
            && let Some(f) = d.filename()
        {
            return Some(f);
        }
        self.content_type.name()
    }

    /// Whether this part qualifies as an attachment (per
    /// [`attachments`](Self::attachments)).
    pub fn is_attachment(&self) -> bool {
        if let Some(d) = &self.disposition
            && d.is_attachment()
        {
            return true;
        }
        if self.content_type.name().is_some()
            || self
                .disposition
                .as_ref()
                .and_then(|d| d.filename())
                .is_some()
        {
            // Has a filename → treat as attachment even without
            // explicit Content-Disposition.
            return true;
        }
        false
    }
}

/// Depth-first iterator over a part tree (yields self first, then
/// children recursively).
fn find_by_ct_recursive<'a>(
    part: &'a Part,
    target_type: &str,
    target_subtype: &str,
) -> Option<&'a Part> {
    if part.content_type.type_.eq_ignore_ascii_case(target_type)
        && part
            .content_type
            .subtype
            .eq_ignore_ascii_case(target_subtype)
    {
        return Some(part);
    }
    for child in &part.children {
        if let Some(found) = find_by_ct_recursive(child, target_type, target_subtype) {
            return Some(found);
        }
    }
    None
}

/// Depth-first iterator over a [`Part`] tree. Constructed by
/// [`Part::walk`]. Visits `self` first then each child in document order.
pub struct Walker<'a> {
    stack: Vec<&'a Part>,
}

impl<'a> Iterator for Walker<'a> {
    type Item = &'a Part;

    fn next(&mut self) -> Option<Self::Item> {
        let p = self.stack.pop()?;
        // Push children in reverse so DFS yields document order.
        for c in p.children.iter().rev() {
            self.stack.push(c);
        }
        Some(p)
    }
}

/// Parse a full message (headers + body) into a Part tree.
///
/// ```rust
/// use mailrs_mime::parse;
/// let raw = b"\
/// Content-Type: multipart/alternative; boundary=\"xx\"\r\n\
/// \r\n\
/// --xx\r\n\
/// Content-Type: text/plain\r\n\
/// \r\n\
/// hello\r\n\
/// --xx\r\n\
/// Content-Type: text/html\r\n\
/// \r\n\
/// <p>hello</p>\r\n\
/// --xx--\r\n";
/// let root = parse(raw);
/// assert!(root.content_type.is_multipart());
/// assert_eq!(root.children.len(), 2);
/// assert_eq!(root.children[0].content_type.mime_type(), "text/plain");
/// assert_eq!(root.children[1].content_type.mime_type(), "text/html");
/// ```
pub fn parse(raw: &[u8]) -> Part {
    let msg = Message::new(raw);

    // Headers parse from `&str`. Use `from_utf8_lossy` only as a
    // fallback for the (rare) non-ASCII case — header values per RFC
    // 5322 are restricted to printable ASCII, so the borrowed branch is
    // the hot path. Avoids 4 small allocations on every typical message.
    let header_str = |name: &str| -> Option<std::borrow::Cow<'_, str>> {
        msg.header(name).map(|v| match std::str::from_utf8(v) {
            Ok(s) => std::borrow::Cow::Borrowed(s),
            Err(_) => std::borrow::Cow::Owned(String::from_utf8_lossy(v).into_owned()),
        })
    };

    let content_type = match header_str("Content-Type") {
        Some(v) => ContentType::parse(&v),
        None => ContentType::default_for_missing_header(),
    };
    let disposition = header_str("Content-Disposition").map(|v| Disposition::parse(&v));
    let content_id =
        header_str("Content-ID").map(|v| v.trim().trim_matches(['<', '>']).to_string());
    let transfer_encoding = header_str("Content-Transfer-Encoding")
        .map(|v| TransferEncoding::parse(&v))
        .unwrap_or(TransferEncoding::SevenBit);

    let body = msg.body().unwrap_or(b"");

    if content_type.is_multipart() {
        let children = match content_type.boundary() {
            Some(b) => split_multipart(body, b),
            None => Vec::new(),
        };
        // Multipart preamble is "rarely interesting" per RFC 2046 §5.1.1;
        // dropping it saves a body.to_vec() of the entire raw payload
        // (often 1KB+) per multipart node. Callers who need the preamble
        // can read it via the original raw bytes.
        Part {
            content_type,
            disposition,
            content_id,
            transfer_encoding,
            body: Vec::new(),
            children,
        }
    } else {
        let decoded = transfer_encoding.decode(body);
        Part {
            content_type,
            disposition,
            content_id,
            transfer_encoding,
            body: decoded,
            children: Vec::new(),
        }
    }
}

/// Split a multipart body by `--<boundary>` markers (RFC 2046 §5.1.1).
fn split_multipart(body: &[u8], boundary: &str) -> Vec<Part> {
    let boundary_bytes = boundary.as_bytes();
    // Boundary token = `--<boundary>`. The close form appends `--` again.
    // No heap allocation for either — we work directly against byte
    // slices, so the same `boundary_bytes` is read multiple times.
    let delim_len = 2 + boundary_bytes.len();
    let close_len = 4 + boundary_bytes.len();

    // Pre-size to 4 — typical multipart/alternative or multipart/mixed
    // has 2-4 parts. Saves the first growth tick.
    let mut parts = Vec::with_capacity(4);
    let mut cursor = 0usize;
    let mut current_start: Option<usize> = None;

    while cursor < body.len() {
        // Find next `--<boundary>` at start of a line via memchr-based
        // search for `\n` (SIMD-vectorised on aarch64/x86_64), then
        // confirm the `--<boundary>` token follows immediately. Was a
        // hand-rolled O(n) byte walk that called the prefix-compare on
        // every offset; this version only compares at confirmed line
        // starts, which is O(n/avg_line_len).
        let next = find_boundary_at_line_start(body, cursor, boundary_bytes);
        let Some(pos) = next else {
            break;
        };
        // If we were inside a part, commit it.
        if let Some(start) = current_start {
            // Strip the CRLF that precedes the boundary marker per
            // RFC 2046 §5.1.1: "The CRLF preceding the boundary
            // delimiter is considered part of the boundary".
            let end = pos.saturating_sub(2);
            let end = if end >= start && &body[end..pos] == b"\r\n" {
                end
            } else if pos > 0 && body[pos - 1] == b'\n' {
                pos - 1
            } else {
                pos
            };
            let part_bytes = &body[start..end];
            parts.push(parse(part_bytes));
        }
        // Is this the close delimiter (`--boundary--`)?
        let close_end = pos + close_len;
        let is_close = close_end <= body.len() && body[pos + delim_len..close_end] == [b'-', b'-'];
        if is_close {
            break;
        }
        // Advance past delim + any trailing WSP/CRLF on the boundary line.
        let mut after = pos + delim_len;
        // Skip any trailing transport-padding WSP
        while after < body.len() && matches!(body[after], b' ' | b'\t') {
            after += 1;
        }
        // Skip CRLF
        if after + 1 < body.len() && body[after] == b'\r' && body[after + 1] == b'\n' {
            after += 2;
        } else if after < body.len() && body[after] == b'\n' {
            after += 1;
        }
        current_start = Some(after);
        cursor = after;
    }

    parts
}

/// Find `--<boundary>` in `body` starting at `cursor`, restricted to
/// matches at the start of a line. Uses `memchr` to jump to candidate
/// line starts (via `\n` byte search) and confirms the `--` prefix +
/// boundary match only at those positions — avoids the per-position
/// pattern compare that the previous hand-rolled walk paid for.
#[inline]
fn find_boundary_at_line_start(body: &[u8], cursor: usize, boundary: &[u8]) -> Option<usize> {
    let delim_len = 2 + boundary.len();
    // Pos-0 special case: line-start without a preceding newline.
    if cursor == 0 && body.len() >= delim_len {
        if body[0] == b'-' && body[1] == b'-' && body[2..delim_len] == *boundary {
            return Some(0);
        }
    }
    // memchr-driven hops over `\n`s. Each hit is a candidate line start
    // at position `nl + 1`; we just need to verify the `--<boundary>`
    // prefix matches there.
    let mut search = cursor;
    while search < body.len() {
        let rel = memchr::memchr(b'\n', &body[search..])?;
        let line_start = search + rel + 1;
        if line_start + delim_len <= body.len()
            && body[line_start] == b'-'
            && body[line_start + 1] == b'-'
            && body[line_start + 2..line_start + delim_len] == *boundary
        {
            return Some(line_start);
        }
        search = line_start;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_text_plain() {
        let raw = b"Content-Type: text/plain\r\n\r\nhello";
        let p = parse(raw);
        assert_eq!(p.content_type.mime_type(), "text/plain");
        assert!(p.children.is_empty());
        assert_eq!(p.body_text().as_deref(), Some("hello"));
    }

    #[test]
    fn parse_no_content_type_defaults_text_plain() {
        let raw = b"From: a\r\n\r\nhello";
        let p = parse(raw);
        assert_eq!(p.content_type.mime_type(), "text/plain");
        assert_eq!(p.body_text().as_deref(), Some("hello"));
    }

    #[test]
    fn parse_multipart_alternative() {
        let raw = b"Content-Type: multipart/alternative; boundary=\"xx\"\r\n\
                    \r\n\
                    --xx\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    hello plain\r\n\
                    --xx\r\n\
                    Content-Type: text/html\r\n\
                    \r\n\
                    <p>hello</p>\r\n\
                    --xx--\r\n";
        let p = parse(raw);
        assert!(p.content_type.is_multipart());
        assert_eq!(p.children.len(), 2);
        assert_eq!(p.children[0].body_text().as_deref(), Some("hello plain"));
        assert_eq!(p.children[1].body_text().as_deref(), Some("<p>hello</p>"));
    }

    #[test]
    fn find_by_content_type_returns_first_match() {
        let raw = b"Content-Type: multipart/alternative; boundary=\"xx\"\r\n\
                    \r\n\
                    --xx\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    plain body\r\n\
                    --xx\r\n\
                    Content-Type: text/html\r\n\
                    \r\n\
                    html body\r\n\
                    --xx--\r\n";
        let p = parse(raw);
        let html = p.find_by_content_type("text/html").unwrap();
        assert_eq!(html.body_text().as_deref(), Some("html body"));
        let plain = p.find_by_content_type("text/plain").unwrap();
        assert_eq!(plain.body_text().as_deref(), Some("plain body"));
        assert!(p.find_by_content_type("text/calendar").is_none());
    }

    #[test]
    fn find_text_calendar_in_invite() {
        // Realistic iTIP shape: multipart/alternative with text/plain,
        // text/html, AND text/calendar parts.
        let raw = b"Content-Type: multipart/alternative; boundary=\"x\"\r\n\
                    \r\n\
                    --x\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    Meeting invite.\r\n\
                    --x\r\n\
                    Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\
                    \r\n\
                    BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n\
                    --x--\r\n";
        let p = parse(raw);
        let cal = p.find_by_content_type("text/calendar").unwrap();
        assert!(cal.body_text().unwrap().contains("VCALENDAR"));
    }

    #[test]
    fn parse_base64_attachment() {
        let raw = b"Content-Type: multipart/mixed; boundary=\"xx\"\r\n\
                    \r\n\
                    --xx\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    See attached.\r\n\
                    --xx\r\n\
                    Content-Type: application/pdf; name=\"report.pdf\"\r\n\
                    Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
                    Content-Transfer-Encoding: base64\r\n\
                    \r\n\
                    SGVsbG8gd29ybGQ=\r\n\
                    --xx--\r\n";
        let p = parse(raw);
        let attachments: Vec<&Part> = p.attachments().collect();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].attachment_filename(), Some("report.pdf"));
        assert_eq!(attachments[0].body, b"Hello world");
    }

    #[test]
    fn parse_quoted_printable_body() {
        let raw = b"Content-Type: text/plain; charset=utf-8\r\n\
                    Content-Transfer-Encoding: quoted-printable\r\n\
                    \r\n\
                    Hello=20world=21";
        let p = parse(raw);
        assert_eq!(p.body_text().as_deref(), Some("Hello world!"));
    }

    #[test]
    fn parse_nested_multipart() {
        // multipart/mixed containing multipart/alternative
        let raw = b"Content-Type: multipart/mixed; boundary=\"outer\"\r\n\
                    \r\n\
                    --outer\r\n\
                    Content-Type: multipart/alternative; boundary=\"inner\"\r\n\
                    \r\n\
                    --inner\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    plain\r\n\
                    --inner\r\n\
                    Content-Type: text/html\r\n\
                    \r\n\
                    <p>html</p>\r\n\
                    --inner--\r\n\
                    --outer\r\n\
                    Content-Type: application/pdf; name=\"x.pdf\"\r\n\
                    Content-Disposition: attachment; filename=\"x.pdf\"\r\n\
                    \r\n\
                    PDFBYTES\r\n\
                    --outer--\r\n";
        let p = parse(raw);
        assert!(p.content_type.is_multipart());
        assert_eq!(p.children.len(), 2);
        // First child is multipart/alternative with 2 leaves
        assert_eq!(p.children[0].children.len(), 2);
        // Second child is the attachment
        let attachments: Vec<&Part> = p.attachments().collect();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].attachment_filename(), Some("x.pdf"));
    }

    #[test]
    fn walk_visits_self_first_then_children_in_order() {
        let raw = b"Content-Type: multipart/alternative; boundary=\"x\"\r\n\
                    \r\n\
                    --x\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    a\r\n\
                    --x\r\n\
                    Content-Type: text/html\r\n\
                    \r\n\
                    b\r\n\
                    --x--\r\n";
        let p = parse(raw);
        let types: Vec<String> = p.walk().map(|x| x.content_type.mime_type()).collect();
        assert_eq!(
            types,
            vec![
                "multipart/alternative".to_string(),
                "text/plain".to_string(),
                "text/html".to_string(),
            ]
        );
    }

    #[test]
    fn content_id_strips_angle_brackets() {
        let raw = b"Content-Type: image/png\r\n\
                    Content-ID: <abc@example.com>\r\n\
                    \r\n\
                    binary";
        let p = parse(raw);
        assert_eq!(p.content_id.as_deref(), Some("abc@example.com"));
    }

    #[test]
    fn empty_body_handled_gracefully() {
        let raw = b"Content-Type: text/plain\r\n\r\n";
        let p = parse(raw);
        assert_eq!(p.body, b"");
    }

    #[test]
    fn is_attachment_via_disposition() {
        let raw = b"Content-Type: text/plain\r\n\
                    Content-Disposition: attachment\r\n\
                    \r\n\
                    hi";
        let p = parse(raw);
        assert!(p.is_attachment());
    }

    #[test]
    fn is_attachment_via_filename_no_disposition() {
        let raw = b"Content-Type: application/pdf; name=\"x.pdf\"\r\n\r\nBYTES";
        let p = parse(raw);
        assert!(p.is_attachment());
        assert_eq!(p.attachment_filename(), Some("x.pdf"));
    }

    #[test]
    fn text_with_iso_2022_jp_charset() {
        // ISO-2022-JP encoding of "テスト" → bytes 1B 24 42 25 46 25 39 25 48 1B 28 42
        let raw = b"Content-Type: text/plain; charset=iso-2022-jp\r\n\
                    \r\n\
                    \x1b$B%F%9%H\x1b(B";
        let p = parse(raw);
        let text = p.body_text().unwrap();
        assert_eq!(text, "テスト");
    }

    #[test]
    fn boundary_with_close_marker_terminates() {
        let raw = b"Content-Type: multipart/mixed; boundary=\"x\"\r\n\
                    \r\n\
                    --x\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    part1\r\n\
                    --x--\r\n\
                    EPILOGUE - SHOULD NOT BE A PART";
        let p = parse(raw);
        assert_eq!(p.children.len(), 1);
        assert_eq!(p.children[0].body_text().as_deref(), Some("part1"));
    }

    #[test]
    fn multipart_without_boundary_yields_no_children() {
        let raw = b"Content-Type: multipart/mixed\r\n\r\nbody";
        let p = parse(raw);
        // No boundary param → can't split → no children
        assert!(p.children.is_empty());
    }
}

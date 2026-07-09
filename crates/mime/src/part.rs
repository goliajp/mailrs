//! MIME tree parser: splits a message into a recursive [`Part`]
//! structure with parsed Content-Type, Content-Transfer-Encoding,
//! and decoded body bytes.

use std::borrow::Cow;

use crate::content_type::{ContentType, Disposition};
use crate::decoder::TransferEncoding;

/// One MIME part. A message is recursively a Part: the top-level
/// message is the root part, multipart bodies have child parts.
///
/// **v4 API**: `Part` is now lifetime-parameterized so leaf bodies
/// can borrow directly from the input slice for the identity transfer
/// encodings (7bit / 8bit / binary). Only Base64 and Quoted-Printable
/// allocate an owned `Vec<u8>`. The `body` field is a `Cow<'a, [u8]>`
/// accordingly. Callers that need an owned copy can call
/// `part.body.to_vec()` or `part.body.into_owned()`.
#[derive(Debug, Clone)]
pub struct Part<'a> {
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
    /// **Decoded** body bytes for leaf parts. `Cow::Borrowed` for the
    /// identity encodings (7bit/8bit/binary — the common case) which
    /// borrow directly from the input slice. `Cow::Owned` for
    /// Base64 / Quoted-Printable. For multipart parts this is an
    /// empty `Cow::Borrowed(&[])`; use `children` instead.
    pub body: Cow<'a, [u8]>,
    /// Child parts for `multipart/*` types. Empty for leaf parts.
    pub children: Vec<Part<'a>>,
}

impl<'a> Part<'a> {
    /// Find the first descendant part (depth-first) matching
    /// `<type>/<subtype>` (lowercased compare). Returns `None` if
    /// no part matches.
    pub fn find_by_content_type(&self, mime_type: &str) -> Option<&Part<'a>> {
        // Split the target once into (type, subtype). `ContentType::parse`
        // already lowercases both halves on the part side, so we lowercase
        // the target *once* here and let the recursive walk do plain `==`
        // comparisons — a 6-byte ASCII memcmp per level instead of an
        // `eq_ignore_ascii_case` byte fold.
        let (target_type, target_subtype) = mime_type.split_once('/')?;
        let tt: String = target_type.to_ascii_lowercase();
        let ts: String = target_subtype.to_ascii_lowercase();
        find_by_ct_recursive(self, &tt, &ts)
    }

    /// Depth-first iterator over self + all descendant parts.
    pub fn walk(&self) -> Walker<'_, 'a> {
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
    pub fn attachments(&self) -> impl Iterator<Item = &Part<'a>> {
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
fn find_by_ct_recursive<'p, 'a>(
    part: &'p Part<'a>,
    target_type: &str,
    target_subtype: &str,
) -> Option<&'p Part<'a>> {
    if part.content_type.type_ == target_type && part.content_type.subtype == target_subtype {
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
///
/// `'p` is the lifetime of the borrow into the tree; `'a` is the
/// lifetime of the input slice the part bodies may borrow.
pub struct Walker<'p, 'a> {
    stack: Vec<&'p Part<'a>>,
}

impl<'p, 'a> Iterator for Walker<'p, 'a> {
    type Item = &'p Part<'a>;

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
pub fn parse(raw: &[u8]) -> Part<'_> {
    // Single-pass header walk: one scan of the header region picks
    // up all 4 MIME headers + the body offset. Previous version
    // called `Message::header()` 4 times + `Message::body()` once,
    // each doing its own O(header-region) scan — 5× redundancy on
    // every Part, which dominates parse cost on multipart messages
    // with many leaves.
    let mh = collect_mime_headers(raw);

    let content_type = match mh.content_type {
        Some(v) => ContentType::parse(&header_bytes_to_str(v)),
        None => ContentType::default_for_missing_header(),
    };
    let disposition = mh
        .disposition
        .map(|v| Disposition::parse(&header_bytes_to_str(v)));
    let content_id = mh.content_id.map(|v| {
        header_bytes_to_str(v)
            .trim()
            .trim_matches(['<', '>'])
            .to_string()
    });
    let transfer_encoding = mh
        .transfer_encoding
        .map(|v| TransferEncoding::parse(&header_bytes_to_str(v)))
        .unwrap_or(TransferEncoding::SevenBit);

    let body: &[u8] = &raw[mh.body_offset..];

    if content_type.is_multipart() {
        let children = match content_type.boundary() {
            Some(b) => split_multipart(body, b),
            None => Vec::new(),
        };
        // Multipart preamble is "rarely interesting" per RFC 2046 §5.1.1;
        // dropping it saves a body copy of the entire raw payload
        // (often 1KB+) per multipart node. Callers who need the preamble
        // can read it via the original raw bytes.
        Part {
            content_type,
            disposition,
            content_id,
            transfer_encoding,
            body: Cow::Borrowed(&[]),
            children,
        }
    } else {
        // `decode` returns Cow::Borrowed(body) for the identity
        // encodings (7bit/8bit/binary/Other — the common case),
        // zero-allocation. Only Base64/Quoted-Printable allocate.
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

/// Coerce a header value byte slice into `Cow<str>`. The borrowed
/// branch is the hot path — header bytes per RFC 5322 §2.2 are 7-bit
/// ASCII, and per RFC 6532 they may also be UTF-8; both are
/// `str::from_utf8` clean. The lossy fallback exists only for
/// real-world malformed messages.
#[inline]
fn header_bytes_to_str(v: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(v) {
        Ok(s) => Cow::Borrowed(s),
        Err(_) => Cow::Owned(String::from_utf8_lossy(v).into_owned()),
    }
}

/// Find the end of the logical header line that starts at `start`,
/// handling RFC 5322 §3.2.2 line folding (continuation lines starting
/// with WSP belong to the same logical header). Inlined from
/// `mailrs-rfc5322` because the helper is `pub(crate)` upstream and
/// shipping a new rfc5322 release just to expose 20 lines isn't worth
/// the version churn for now.
///
/// Returns `Some((line_end, after_crlf))` where `line_end` is the
/// content end (before the terminating CR/LF) and `after_crlf` is the
/// next line's start. Returns `None` if no terminator is found.
#[inline]
fn find_unfolded_line_end(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut i = start;
    while i < bytes.len() {
        let lf = memchr::memchr(b'\n', &bytes[i..])?;
        let lf_abs = i + lf;
        let mut content_end = lf_abs;
        if content_end > start && bytes[content_end - 1] == b'\r' {
            content_end -= 1;
        }
        let next = lf_abs + 1;
        if next < bytes.len() && (bytes[next] == b' ' || bytes[next] == b'\t') {
            i = next;
            continue;
        }
        return Some((content_end, next));
    }
    None
}

/// All four MIME-relevant headers + the body offset, collected in one
/// pass over the header region. Each `Option<&[u8]>` is the raw header
/// value (after the colon + one optional WSP, folding preserved). The
/// `body_offset` is the index of the first body byte (or `raw.len()`
/// when the message has no body separator).
struct MimeHeaders<'a> {
    content_type: Option<&'a [u8]>,
    disposition: Option<&'a [u8]>,
    content_id: Option<&'a [u8]>,
    transfer_encoding: Option<&'a [u8]>,
    body_offset: usize,
}

/// Walk the header region exactly once, dispatching each line by its
/// length-at-colon to one of the four MIME slots. Cheaper than five
/// independent `Message::header()` / `Message::body()` scans (which is
/// what naive code does), and the dispatch is branchless after the
/// first-byte 'C'/'c' reject.
fn collect_mime_headers(raw: &[u8]) -> MimeHeaders<'_> {
    let mut out = MimeHeaders {
        content_type: None,
        disposition: None,
        content_id: None,
        transfer_encoding: None,
        body_offset: raw.len(),
    };
    let mut cursor = 0usize;
    while cursor < raw.len() {
        // Empty line terminates header block.
        if raw[cursor] == b'\n' {
            out.body_offset = cursor + 1;
            return out;
        }
        if raw[cursor] == b'\r' && cursor + 1 < raw.len() && raw[cursor + 1] == b'\n' {
            out.body_offset = cursor + 2;
            return out;
        }

        let Some((line_end, after_crlf)) = find_unfolded_line_end(raw, cursor) else {
            // EOF before any body separator — no body.
            return out;
        };
        let line = &raw[cursor..line_end];
        cursor = after_crlf;

        // All four MIME headers start with C/c followed by o/O. Cheap
        // reject before doing the case-insensitive memcmp on the name.
        if line.len() < 11 {
            continue;
        }
        let b0 = line[0];
        let b1 = line[1];
        if !((b0 == b'C' || b0 == b'c') && (b1 == b'O' || b1 == b'o')) {
            continue;
        }

        // Dispatch by name-length-at-colon. Each branch: O(name_len)
        // case-insensitive compare on a byte slice (LLVM lowers this
        // to a vectorised memcmp-with-folding loop).
        let _ = try_dispatch(line, b"Content-Type", &mut out.content_type)
            || try_dispatch(line, b"Content-Disposition", &mut out.disposition)
            || try_dispatch(line, b"Content-ID", &mut out.content_id)
            || try_dispatch(
                line,
                b"Content-Transfer-Encoding",
                &mut out.transfer_encoding,
            );
    }
    out
}

/// If `line` matches `name:` (case-insensitive), capture the value
/// portion into `slot` (unless already filled) and return `true`.
#[inline]
fn try_dispatch<'a>(line: &'a [u8], name: &[u8], slot: &mut Option<&'a [u8]>) -> bool {
    let n = name.len();
    if line.len() > n && line[n] == b':' && line[..n].eq_ignore_ascii_case(name) {
        if slot.is_none() {
            let mut vs = n + 1;
            if vs < line.len() && (line[vs] == b' ' || line[vs] == b'\t') {
                vs += 1;
            }
            *slot = Some(&line[vs..]);
        }
        return true;
    }
    false
}

/// Split a multipart body by `--<boundary>` markers (RFC 2046 §5.1.1).
fn split_multipart<'a>(body: &'a [u8], boundary: &str) -> Vec<Part<'a>> {
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
        let is_close = close_end <= body.len() && body[pos + delim_len..close_end] == *b"--";
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
    if cursor == 0
        && body.len() >= delim_len
        && body[0] == b'-'
        && body[1] == b'-'
        && body[2..delim_len] == *boundary
    {
        return Some(0);
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
        assert_eq!(&*attachments[0].body, b"Hello world");
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
        assert_eq!(&*p.body, b"");
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

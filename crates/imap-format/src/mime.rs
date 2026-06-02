//! MIME walk + header/body section extraction.
//!
//! Pure helpers for splitting a raw RFC 5322 message into its
//! header block + body, parsing Content-Type / Content-Transfer-
//! Encoding / Content-Disposition into [`MimeInfo`], splitting a
//! multipart body by boundary, and extracting a specific MIME
//! part by section number (`"1"`, `"1.MIME"`, `"1.1"`, ...).

/// extract only the specified header fields from raw message
pub fn extract_header_fields(data: &[u8], fields: &[String]) -> Vec<u8> {
    let header = extract_header_section(data);
    let header_str = String::from_utf8_lossy(&header);
    let mut result = Vec::new();
    let mut include = false;
    for line in header_str.lines() {
        if line.is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if include {
                result.extend_from_slice(line.as_bytes());
                result.extend_from_slice(b"\r\n");
            }
        } else {
            include = false;
            if let Some(colon) = line.find(':') {
                let name = line[..colon].trim().to_uppercase();
                if fields.contains(&name) {
                    include = true;
                    result.extend_from_slice(line.as_bytes());
                    result.extend_from_slice(b"\r\n");
                }
            }
        }
    }
    result.extend_from_slice(b"\r\n");
    result
}

/// Find the end of the header section: returns the byte offset
/// **after** the last byte of the header/body separator (so the
/// header occupies `[..end]` and the body occupies `[end..]`).
///
/// Recognises `\r\n\r\n` (canonical) and `\n\n` (bare-LF MTAs).
/// Anchors on `\n` via memchr — LF is rare in textual headers so
/// the SIMD scan jumps over the bulk of the input. Replaces the
/// previous `windows(4).position` / `windows(2).position` scans
/// (per-byte loop) and is the hot path for every IMAP
/// `FETCH BODY[HEADER]` / `FETCH BODY[TEXT]`.
fn find_separator_end(data: &[u8]) -> Option<usize> {
    let mut search = 0;
    while let Some(rel) = memchr::memchr(b'\n', &data[search..]) {
        let pos = search + rel;
        // CRLF CRLF — pos is the second LF, [pos-3..=pos] == \r\n\r\n
        if pos >= 3 && &data[pos - 3..=pos] == b"\r\n\r\n" {
            return Some(pos + 1);
        }
        // LF LF — pos is the second LF, previous byte also LF
        if pos >= 1 && data[pos - 1] == b'\n' {
            return Some(pos + 1);
        }
        search = pos + 1;
    }
    None
}

/// extract header section from raw message (up to \r\n\r\n)
pub fn extract_header_section(data: &[u8]) -> Vec<u8> {
    match find_separator_end(data) {
        Some(end) => data[..end].to_vec(),
        None => data.to_vec(),
    }
}

/// extract body section from raw message (after \r\n\r\n)
pub fn extract_body_section(data: &[u8]) -> Vec<u8> {
    match find_separator_end(data) {
        Some(end) => data[end..].to_vec(),
        None => Vec::new(),
    }
}

/// Parsed `Content-Type` + `Content-Transfer-Encoding` +
/// `Content-Disposition` info extracted from a single MIME part's
/// header block. Returned by [`parse_mime_headers`].
pub struct MimeInfo {
    /// `Content-Type` major type ("TEXT", "MULTIPART", "APPLICATION", ...). Uppercase.
    pub media_type: String,
    /// `Content-Type` subtype ("PLAIN", "HTML", "MIXED", "PDF", ...). Uppercase.
    pub subtype: String,
    /// `Content-Type` `charset=` parameter. Defaults to `"UTF-8"`.
    pub charset: String,
    /// `Content-Transfer-Encoding` ("7BIT", "BASE64", "QUOTED-PRINTABLE", "8BIT"). Defaults to `"7BIT"`.
    pub encoding: String,
    /// `Content-Type` `boundary=` parameter for multipart bodies. `None` for non-multipart.
    pub boundary: Option<String>,
    /// `Content-Type` `name=` parameter (legacy way to name an attachment).
    pub name: Option<String>,
    /// `Content-ID` value with `<…>` brackets stripped.
    pub content_id: Option<String>,
    /// `Content-Disposition` value: `"attachment"` or `"inline"`. `None` if absent.
    pub disposition: Option<String>,
    /// `Content-Disposition` `filename=` parameter — preferred over `name=` when present.
    pub disposition_filename: Option<String>,
}

/// parse content-type and transfer-encoding from header text
pub fn parse_mime_headers(header: &str) -> MimeInfo {
    let mut media_type = "TEXT".to_string();
    let mut subtype = "PLAIN".to_string();
    let mut charset = "UTF-8".to_string();
    let mut encoding = "7BIT".to_string();
    let mut boundary = None;
    let mut name = None;
    let mut content_id = None;
    let mut disposition = None;
    let mut disposition_filename = None;

    // unfold headers (join continuation lines)
    let mut unfolded = String::new();
    for line in header.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            unfolded.push(' ');
            unfolded.push_str(line.trim());
        } else {
            if !unfolded.is_empty() {
                unfolded.push('\n');
            }
            unfolded.push_str(line);
        }
    }

    for line in unfolded.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("content-type:") {
            let val = line["content-type:".len()..].trim();
            let val_lower = val.to_lowercase();
            if val_lower.starts_with("text/html") || val_lower.contains("text/html") {
                media_type = "TEXT".to_string();
                subtype = "HTML".to_string();
            } else if val_lower.starts_with("text/plain") || val_lower.contains("text/plain") {
                media_type = "TEXT".to_string();
                subtype = "PLAIN".to_string();
            } else if val_lower.contains("multipart/") {
                media_type = "MULTIPART".to_string();
                if val_lower.contains("multipart/mixed") {
                    subtype = "MIXED".to_string();
                } else if val_lower.contains("multipart/alternative") {
                    subtype = "ALTERNATIVE".to_string();
                } else if val_lower.contains("multipart/related") {
                    subtype = "RELATED".to_string();
                } else {
                    subtype = "MIXED".to_string();
                }
            } else if val_lower.contains("application/") {
                media_type = "APPLICATION".to_string();
                if let Some(s) = val_lower.split('/').nth(1) {
                    subtype = s
                        .split(';')
                        .next()
                        .unwrap_or("OCTET-STREAM")
                        .trim()
                        .to_uppercase();
                }
            } else if val_lower.contains("image/") {
                media_type = "IMAGE".to_string();
                if let Some(s) = val_lower.split('/').nth(1) {
                    subtype = s.split(';').next().unwrap_or("JPEG").trim().to_uppercase();
                }
            }
            // extract charset
            if let Some(pos) = val_lower.find("charset=") {
                let rest = &val[pos + 8..];
                let cs = rest
                    .trim_start_matches('"')
                    .split(|c: char| c == '"' || c == ';' || c.is_whitespace())
                    .next()
                    .unwrap_or("UTF-8");
                charset = cs.to_uppercase();
            }
            // extract name
            if let Some(pos) = val_lower.find("name=") {
                let rest = &val[pos + 5..];
                let n = if let Some(stripped) = rest.strip_prefix('"') {
                    stripped.split('"').next().unwrap_or("")
                } else {
                    rest.split(|c: char| c == ';' || c.is_whitespace())
                        .next()
                        .unwrap_or("")
                };
                if !n.is_empty() {
                    name = Some(n.to_string());
                }
            }
            // extract boundary
            if let Some(pos) = val_lower.find("boundary=") {
                let rest = &val[pos + 9..];
                let b = if let Some(stripped) = rest.strip_prefix('"') {
                    stripped.split('"').next().unwrap_or("")
                } else {
                    rest.split(|c: char| c == ';' || c.is_whitespace())
                        .next()
                        .unwrap_or("")
                };
                boundary = Some(b.to_string());
            }
        }
        if lower.starts_with("content-transfer-encoding:") {
            let val = line["content-transfer-encoding:".len()..].trim();
            encoding = match val.to_uppercase().as_str() {
                "BASE64" => "BASE64".to_string(),
                "QUOTED-PRINTABLE" => "QUOTED-PRINTABLE".to_string(),
                "8BIT" => "8BIT".to_string(),
                _ => "7BIT".to_string(),
            };
        }
        if lower.starts_with("content-id:") {
            let val = line["content-id:".len()..].trim();
            content_id = Some(val.trim_matches(|c| c == '<' || c == '>').to_string());
        }
        if lower.starts_with("content-disposition:") {
            let val = line["content-disposition:".len()..].trim();
            let val_lower = val.to_lowercase();
            if val_lower.starts_with("attachment") {
                disposition = Some("attachment".to_string());
            } else if val_lower.starts_with("inline") {
                disposition = Some("inline".to_string());
            }
            if let Some(pos) = val_lower.find("filename=") {
                let rest = &val[pos + 9..];
                let f = if let Some(stripped) = rest.strip_prefix('"') {
                    stripped.split('"').next().unwrap_or("")
                } else {
                    rest.split(|c: char| c == ';' || c.is_whitespace())
                        .next()
                        .unwrap_or("")
                };
                if !f.is_empty() {
                    disposition_filename = Some(f.to_string());
                }
            }
        }
    }

    MimeInfo {
        media_type,
        subtype,
        charset,
        encoding,
        boundary,
        name,
        content_id,
        disposition,
        disposition_filename,
    }
}

/// split multipart body by boundary, returning each part as raw bytes (including part headers)
pub fn split_mime_parts<'a>(body: &'a [u8], boundary: &str) -> Vec<&'a [u8]> {
    let delim = format!("--{boundary}");
    let body_str = String::from_utf8_lossy(body);
    let mut parts = Vec::new();

    let mut in_parts = false;
    let mut part_start = 0;

    for (i, line) in body_str.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(&delim) {
            if trimmed == format!("--{boundary}--")
                || trimmed.starts_with(&format!("--{boundary}--"))
            {
                if in_parts
                    && let Some(pos) = find_line_offset(body, i)
                    && pos > part_start
                {
                    parts.push(&body[part_start..pos]);
                }
                break;
            }
            if in_parts
                && let Some(pos) = find_line_offset(body, i)
                && pos > part_start
            {
                parts.push(&body[part_start..pos]);
            }
            if let Some(pos) = find_line_offset(body, i) {
                let after = pos + line.len();
                part_start =
                    if body.get(after) == Some(&b'\r') && body.get(after + 1) == Some(&b'\n') {
                        after + 2
                    } else if body.get(after) == Some(&b'\n') {
                        after + 1
                    } else {
                        after
                    };
            }
            in_parts = true;
        }
    }

    parts
}

/// find byte offset of line number in body. Each LF advances the
/// line counter; the returned offset is the byte immediately after
/// the LF (i.e. the first byte of the next line). memchr replaces
/// the previous byte-by-byte `iter().position` scan.
pub fn find_line_offset(body: &[u8], target_line: usize) -> Option<usize> {
    let mut line_num = 0;
    let mut pos = 0;
    while pos < body.len() {
        if line_num == target_line {
            return Some(pos);
        }
        match memchr::memchr(b'\n', &body[pos..]) {
            Some(nl) => pos = pos + nl + 1,
            None => pos = body.len(),
        }
        line_num += 1;
    }
    if line_num == target_line {
        Some(pos)
    } else {
        None
    }
}

/// trim trailing CRLF/LF from part body (boundary transport padding per RFC 2046)
pub fn trim_part_trailing_newline(data: &[u8]) -> &[u8] {
    let mut end = data.len();
    if end >= 2 && data[end - 2] == b'\r' && data[end - 1] == b'\n' {
        end -= 2;
    } else if end >= 1 && data[end - 1] == b'\n' {
        end -= 1;
    }
    &data[..end]
}

/// extract a specific MIME part by number (e.g. "1", "2", "1.1", "1.MIME")
pub fn extract_mime_part(data: &[u8], section: &str) -> Option<Vec<u8>> {
    let upper = section.to_uppercase();
    if upper.ends_with(".MIME") {
        let base = &section[..section.len() - 5];
        let part_raw = find_mime_part_raw(data, base)?;
        return Some(extract_header_section(&part_raw));
    }

    find_mime_part_body(data, section)
}

/// find a MIME part's raw data (headers + body) by section number
fn find_mime_part_raw(data: &[u8], section: &str) -> Option<Vec<u8>> {
    let header_bytes = extract_header_section(data);
    let header = String::from_utf8_lossy(&header_bytes);
    let body = extract_body_section(data);
    let info = parse_mime_headers(&header);

    if info.media_type != "MULTIPART" || info.boundary.is_none() {
        if section == "1" {
            return Some(data.to_vec());
        }
        return None;
    }

    let boundary = info.boundary.as_ref()?;
    let parts = split_mime_parts(&body, boundary);

    let mut parts_iter = section.split('.');
    let first: usize = parts_iter.next()?.parse().ok()?;
    let rest: String = parts_iter.collect::<Vec<_>>().join(".");

    if first == 0 || first > parts.len() {
        return None;
    }

    let part = parts[first - 1];
    if rest.is_empty() {
        Some(part.to_vec())
    } else {
        find_mime_part_raw(part, &rest)
    }
}

/// extract a specific MIME part's body by section number (e.g. "1", "2", "1.1")
fn find_mime_part_body(data: &[u8], section: &str) -> Option<Vec<u8>> {
    let header_bytes = extract_header_section(data);
    let header = String::from_utf8_lossy(&header_bytes);
    let body = extract_body_section(data);
    let info = parse_mime_headers(&header);

    if info.media_type != "MULTIPART" || info.boundary.is_none() {
        if section == "1" {
            return Some(body);
        }
        return None;
    }

    let boundary = info.boundary.as_ref()?;
    let parts = split_mime_parts(&body, boundary);

    let mut parts_iter = section.split('.');
    let first: usize = parts_iter.next()?.parse().ok()?;
    let rest: String = parts_iter.collect::<Vec<_>>().join(".");

    if first == 0 || first > parts.len() {
        return None;
    }

    let part = parts[first - 1];
    if rest.is_empty() {
        Some(extract_body_section(part))
    } else {
        find_mime_part_body(part, &rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_header_section_crlf() {
        let msg = b"From: a@b\r\nSubject: hi\r\n\r\nBody here";
        let header = extract_header_section(msg);
        assert!(header.ends_with(b"\r\n\r\n"));
        assert!(!header.windows(4).any(|w| w == b"Body"));
    }

    #[test]
    fn extract_body_section_crlf() {
        let msg = b"From: a@b\r\n\r\nBody here";
        let body = extract_body_section(msg);
        assert_eq!(body, b"Body here");
    }

    #[test]
    fn extract_header_fields_filters() {
        let msg = b"From: a@b\r\nTo: c@d\r\nSubject: hi\r\n\r\nBody";
        let fields = vec!["FROM".to_string()];
        let result = extract_header_fields(msg, &fields);
        let s = String::from_utf8_lossy(&result);
        assert!(s.contains("From: a@b"));
        assert!(!s.contains("To:"));
        assert!(!s.contains("Subject:"));
    }

    #[test]
    fn parse_mime_headers_text_plain() {
        let info = parse_mime_headers("Content-Type: text/plain; charset=ISO-8859-1\r\n");
        assert_eq!(info.media_type, "TEXT");
        assert_eq!(info.subtype, "PLAIN");
        assert_eq!(info.charset, "ISO-8859-1");
    }

    #[test]
    fn parse_mime_headers_multipart() {
        let info = parse_mime_headers("Content-Type: multipart/mixed; boundary=\"abc123\"\r\n");
        assert_eq!(info.media_type, "MULTIPART");
        assert_eq!(info.subtype, "MIXED");
        assert_eq!(info.boundary, Some("abc123".to_string()));
    }

    #[test]
    fn parse_mime_headers_attachment() {
        let h = "Content-Type: application/pdf; name=\"doc.pdf\"\r\nContent-Disposition: attachment; filename=\"doc.pdf\"\r\n";
        let info = parse_mime_headers(h);
        assert_eq!(info.media_type, "APPLICATION");
        assert_eq!(info.name, Some("doc.pdf".to_string()));
        assert_eq!(info.disposition, Some("attachment".to_string()));
        assert_eq!(info.disposition_filename, Some("doc.pdf".to_string()));
    }

    #[test]
    fn extract_mime_part_single() {
        let msg = b"Content-Type: text/plain\r\n\r\nHello";
        let part = extract_mime_part(msg, "1").unwrap();
        assert_eq!(part, b"Hello");
    }

    #[test]
    fn extract_mime_part_out_of_range() {
        let msg = b"Content-Type: text/plain\r\n\r\nHello";
        assert!(extract_mime_part(msg, "2").is_none());
    }

    #[test]
    fn extract_header_section_lf_only() {
        let msg = b"From: a@b\nSubject: hi\n\nBody";
        let header = extract_header_section(msg);
        assert!(header.ends_with(b"\n\n"));
        assert!(!header.windows(4).any(|w| w == b"Body"));
    }

    #[test]
    fn extract_header_section_no_delimiter_returns_all() {
        let msg = b"From: a@b\nSubject: hi";
        let header = extract_header_section(msg);
        assert_eq!(header, msg);
    }

    #[test]
    fn extract_body_section_lf_only() {
        let msg = b"From: a@b\n\nBody here";
        assert_eq!(extract_body_section(msg), b"Body here");
    }

    #[test]
    fn extract_body_section_no_delimiter_empty() {
        assert!(extract_body_section(b"no body marker").is_empty());
    }

    #[test]
    fn extract_header_fields_continuation_line() {
        // RFC 5322 §3.2.2 folded line — the Subject value continues on the next
        // line starting with a space. extract_header_fields must include both
        // lines when the header is selected.
        let msg = b"Subject: first line\r\n part two\r\nFrom: a@b\r\n\r\nbody";
        let result = extract_header_fields(msg, &["SUBJECT".to_string()]);
        let s = String::from_utf8_lossy(&result);
        assert!(s.contains("Subject: first line"));
        assert!(s.contains(" part two"));
        assert!(!s.contains("From:"));
    }

    #[test]
    fn parse_mime_headers_text_html() {
        let info = parse_mime_headers("Content-Type: text/html; charset=UTF-8\r\n");
        assert_eq!(info.media_type, "TEXT");
        assert_eq!(info.subtype, "HTML");
    }

    #[test]
    fn parse_mime_headers_alternative() {
        let info =
            parse_mime_headers("Content-Type: multipart/alternative; boundary=\"alt-bound\"\r\n");
        assert_eq!(info.subtype, "ALTERNATIVE");
        assert_eq!(info.boundary, Some("alt-bound".to_string()));
    }

    #[test]
    fn parse_mime_headers_related() {
        let info = parse_mime_headers("Content-Type: multipart/related; boundary=rel-bound\r\n");
        assert_eq!(info.subtype, "RELATED");
    }

    #[test]
    fn parse_mime_headers_unknown_multipart_defaults_to_mixed() {
        let info = parse_mime_headers("Content-Type: multipart/encrypted; boundary=enc\r\n");
        assert_eq!(info.media_type, "MULTIPART");
        assert_eq!(info.subtype, "MIXED");
    }

    #[test]
    fn parse_mime_headers_image() {
        let info = parse_mime_headers("Content-Type: image/jpeg; name=\"photo.jpg\"\r\n");
        assert_eq!(info.media_type, "IMAGE");
        assert_eq!(info.subtype, "JPEG");
        assert_eq!(info.name, Some("photo.jpg".to_string()));
    }

    #[test]
    fn parse_mime_headers_inline_disposition() {
        let info = parse_mime_headers("Content-Disposition: inline; filename=\"cid.png\"\r\n");
        assert_eq!(info.disposition, Some("inline".to_string()));
        assert_eq!(info.disposition_filename, Some("cid.png".to_string()));
    }

    #[test]
    fn parse_mime_headers_content_id_strips_brackets() {
        let info = parse_mime_headers("Content-Id: <abc@xyz>\r\n");
        assert_eq!(info.content_id, Some("abc@xyz".to_string()));
    }

    #[test]
    fn parse_mime_headers_transfer_encoding_variants() {
        for (input, want) in [
            ("Content-Transfer-Encoding: base64\r\n", "BASE64"),
            (
                "Content-Transfer-Encoding: Quoted-Printable\r\n",
                "QUOTED-PRINTABLE",
            ),
            ("Content-Transfer-Encoding: 8bit\r\n", "8BIT"),
            ("Content-Transfer-Encoding: 7BIT\r\n", "7BIT"),
            ("Content-Transfer-Encoding: weirdo\r\n", "7BIT"),
        ] {
            assert_eq!(parse_mime_headers(input).encoding, want, "{input}");
        }
    }

    #[test]
    fn parse_mime_headers_folded_header() {
        let header = "Content-Type: multipart/mixed;\r\n boundary=\"folded-bound\"\r\n";
        let info = parse_mime_headers(header);
        assert_eq!(info.media_type, "MULTIPART");
        assert_eq!(info.subtype, "MIXED");
        assert_eq!(info.boundary, Some("folded-bound".to_string()));
    }

    #[test]
    fn split_mime_parts_two_parts() {
        let body = b"--bound\r\nContent-Type: text/plain\r\n\r\npart1\r\n--bound\r\nContent-Type: text/html\r\n\r\npart2\r\n--bound--\r\n";
        let parts = split_mime_parts(body, "bound");
        assert_eq!(parts.len(), 2, "expected exactly 2 parts");
        let p0 = String::from_utf8_lossy(parts[0]);
        let p1 = String::from_utf8_lossy(parts[1]);
        assert!(p0.contains("part1"));
        assert!(p1.contains("part2"));
    }

    #[test]
    fn split_mime_parts_missing_terminator() {
        // No --bound-- closing delim — still returns the parts seen so far.
        let body = b"--b\r\nfoo\r\n--b\r\nbar\r\n";
        let parts = split_mime_parts(body, "b");
        assert!(!parts.is_empty());
    }

    #[test]
    fn find_line_offset_first_line() {
        let body = b"line0\r\nline1\r\nline2\r\n";
        assert_eq!(find_line_offset(body, 0), Some(0));
        assert_eq!(find_line_offset(body, 1), Some(7));
        assert_eq!(find_line_offset(body, 2), Some(14));
    }

    #[test]
    fn find_line_offset_out_of_range() {
        let body = b"a\r\nb\r\n";
        assert_eq!(find_line_offset(body, 100), None);
    }

    #[test]
    fn trim_part_trailing_newline_crlf() {
        let s = b"hello\r\n";
        assert_eq!(trim_part_trailing_newline(s), b"hello");
    }

    #[test]
    fn trim_part_trailing_newline_lf_only() {
        let s = b"hello\n";
        assert_eq!(trim_part_trailing_newline(s), b"hello");
    }

    #[test]
    fn trim_part_trailing_newline_no_newline() {
        let s = b"hello";
        assert_eq!(trim_part_trailing_newline(s), b"hello");
    }

    #[test]
    fn extract_mime_part_nested_multipart() {
        let body = concat!(
            "Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n",
            "--outer\r\n",
            "Content-Type: text/plain\r\n\r\n",
            "alpha\r\n",
            "--outer\r\n",
            "Content-Type: text/html\r\n\r\n",
            "<p>beta</p>\r\n",
            "--outer--\r\n",
        )
        .as_bytes();
        let p1 = extract_mime_part(body, "1").expect("part 1 exists");
        assert!(String::from_utf8_lossy(&p1).contains("alpha"));
        let p2 = extract_mime_part(body, "2").expect("part 2 exists");
        assert!(String::from_utf8_lossy(&p2).contains("beta"));
    }
}

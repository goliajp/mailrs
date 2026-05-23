//! IMAP wire-format helpers (RFC 9051 §6.4 FETCH responses, §7.5
//! BODYSTRUCTURE assembly, §9 ABNF for FLAGS / INTERNALDATE).
//!
//! Pairs with [`mailrs-imap-proto`](https://crates.io/crates/mailrs-imap-proto)
//! (command parsing + session state machine) and
//! [`mailrs-imap-codec`](https://crates.io/crates/mailrs-imap-codec)
//! (line / literal framing) to form a complete RFC 9051 receive +
//! response stack.
//!
//! 22 standalone helpers grouped by concern:
//!
//! - **FLAGS** — `format_imap_flags` / `parse_imap_flags` map the 6
//!   standard system flags between `u32` bitmask and IMAP
//!   `(\Seen \Flagged …)` syntax. Bit assignments are exposed via
//!   the `FLAG_*` `pub const` so callers can construct masks
//!   without round-tripping through strings.
//! - **INTERNALDATE** — `format_internal_date(i64)` formats a Unix
//!   timestamp as IMAP's `"DD-Mon-YYYY HH:MM:SS +ZZZZ"`.
//! - **String quoting** — `escape_imap_string` / `escape_imap_str`
//!   / `quote_or_nil` handle IMAP's `"…"` quoted-string with
//!   `\`-escapes-or-`NIL` rules.
//! - **Address parsing** — `format_imap_address` + `format_addr_list`
//!   turn RFC 5322 addresses (with optional display name) into
//!   IMAP's `((name route mailbox host))` structure.
//! - **BODY[] section requests** — `parse_header_fields_request`
//!   (`BODY[HEADER.FIELDS (…)]`) + `parse_generic_body_sections`
//!   (`BODY[1]`, `BODY[1.MIME]`, etc.).
//! - **MIME walk** — `extract_header_section` / `extract_body_section`
//!   / `extract_header_fields` / `parse_mime_headers` (returns
//!   [`MimeInfo`]) / `split_mime_parts` / `find_line_offset` /
//!   `trim_part_trailing_newline` / `extract_mime_part`.
//! - **BODYSTRUCTURE** — `build_bodystructure` recurses through
//!   multipart trees and emits the RFC-9051 §7.5.2 form.
//!
//! All helpers are pure functions — no I/O, no async.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

/// IMAP `\Seen` system flag. Bit-0 of the standard u32 mask.
pub const FLAG_SEEN: u32 = 0b0000_0001;
/// IMAP `\Answered` system flag. Bit-1.
pub const FLAG_ANSWERED: u32 = 0b0000_0010;
/// IMAP `\Flagged` system flag (star). Bit-2.
pub const FLAG_FLAGGED: u32 = 0b0000_0100;
/// IMAP `\Deleted` system flag (queued for expunge). Bit-3.
pub const FLAG_DELETED: u32 = 0b0000_1000;
/// IMAP `\Draft` system flag. Bit-4.
pub const FLAG_DRAFT: u32 = 0b0001_0000;
/// IMAP `\Recent` system flag (set by EXAMINE/SELECT). Bit-5.
pub const FLAG_RECENT: u32 = 0b0010_0000;

/// convert bitmask flags to IMAP flag string
pub fn format_imap_flags(flags: u32) -> String {
    let mut parts = Vec::new();
    if flags & FLAG_SEEN != 0 {
        parts.push("\\Seen");
    }
    if flags & FLAG_ANSWERED != 0 {
        parts.push("\\Answered");
    }
    if flags & FLAG_FLAGGED != 0 {
        parts.push("\\Flagged");
    }
    if flags & FLAG_DELETED != 0 {
        parts.push("\\Deleted");
    }
    if flags & FLAG_DRAFT != 0 {
        parts.push("\\Draft");
    }
    if flags & FLAG_RECENT != 0 {
        parts.push("\\Recent");
    }
    parts.join(" ")
}

/// parse IMAP flag names from a FLAGS string like "(\\Seen \\Flagged)"
pub fn parse_imap_flags(s: &str) -> u32 {
    let s = s.trim().trim_start_matches('(').trim_end_matches(')');
    let mut bits = 0u32;
    for part in s.split_whitespace() {
        let flag = part.trim_start_matches('\\');
        match flag.to_uppercase().as_str() {
            "SEEN" => bits |= FLAG_SEEN,
            "ANSWERED" => bits |= FLAG_ANSWERED,
            "FLAGGED" => bits |= FLAG_FLAGGED,
            "DELETED" => bits |= FLAG_DELETED,
            "DRAFT" => bits |= FLAG_DRAFT,
            "RECENT" => bits |= FLAG_RECENT,
            _ => {}
        }
    }
    bits
}

/// Format a Unix timestamp as an IMAP `INTERNALDATE` value
/// (`"DD-Mon-YYYY HH:MM:SS +ZZZZ"`, RFC 9051 §9 ABNF
/// `date-time`).
pub fn format_internal_date(timestamp: i64) -> String {
    use chrono::DateTime;
    let dt = DateTime::from_timestamp(timestamp, 0).unwrap_or_default();
    dt.format("%d-%b-%Y %H:%M:%S %z").to_string()
}

/// Escape `\` and `"` characters for embedding inside an IMAP
/// `"…"` quoted string. Does NOT add the surrounding quotes —
/// see [`quote_or_nil`] for the full quoted-or-`NIL` decision.
pub fn escape_imap_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// quote a string for IMAP or return NIL if empty
pub fn quote_or_nil(s: &str) -> String {
    if s.is_empty() {
        "NIL".to_string()
    } else {
        format!("\"{}\"", escape_imap_string(s))
    }
}

/// parse an email address "Name <user@host>" or "user@host" into IMAP address structure
/// returns ((name NIL mailbox host)) or NIL if empty
pub fn format_imap_address(addr: &str) -> String {
    let addr = addr.trim();
    if addr.is_empty() {
        return "NIL".to_string();
    }

    // parse "Name <user@host>" format
    if let Some(lt) = addr.find('<') {
        let name = addr[..lt].trim().trim_matches('"');
        let email = addr[lt + 1..].trim_end_matches('>');
        let (mailbox, host) = email.split_once('@').unwrap_or((email, ""));
        let name_part = if name.is_empty() {
            "NIL".to_string()
        } else {
            format!("\"{}\"", escape_imap_string(name))
        };
        return format!(
            "(({name_part} NIL \"{}\" \"{}\"))",
            escape_imap_string(mailbox),
            escape_imap_string(host)
        );
    }

    // plain "user@host"
    if let Some((mailbox, host)) = addr.split_once('@') {
        format!(
            "((NIL NIL \"{}\" \"{}\"))",
            escape_imap_string(mailbox),
            escape_imap_string(host)
        )
    } else {
        format!("((NIL NIL \"{}\" \"\"))", escape_imap_string(addr))
    }
}

/// parse BODY[HEADER.FIELDS (field-list)] or BODY.PEEK[HEADER.FIELDS (field-list)]
/// returns (field_names, raw_section_text)
pub fn parse_header_fields_request(attributes: &str) -> Option<(Vec<String>, String)> {
    let upper = attributes.to_uppercase();
    let marker = "HEADER.FIELDS";
    let pos = upper.find(marker)?;
    let after = &attributes[pos + marker.len()..];
    let paren_start = after.find('(')?;
    let paren_end = after.find(')')?;
    let fields_str = &after[paren_start + 1..paren_end];
    let fields: Vec<String> = fields_str
        .split_whitespace()
        .map(|s| s.to_uppercase())
        .collect();
    let raw_section = format!("HEADER.FIELDS ({})", fields_str.trim());
    Some((fields, raw_section))
}

/// parse all generic BODY[section] requests like BODY[1], BODY[1.1], BODY[1.MIME], BODY.PEEK[1]
/// returns all section specifiers (e.g. ["1", "1.1", "1.MIME"])
pub fn parse_generic_body_sections(attributes: &str) -> Vec<String> {
    let upper = attributes.to_uppercase();
    let mut sections = Vec::new();

    for prefix in &["BODY.PEEK[", "BODY["] {
        let mut search_from = 0;
        while let Some(rel_pos) = upper[search_from..].find(prefix) {
            let abs_start = search_from + rel_pos + prefix.len();
            if let Some(end_rel) = upper[abs_start..].find(']') {
                let section = attributes[abs_start..abs_start + end_rel].trim();
                let sec_upper = section.to_uppercase();
                if !section.is_empty()
                    && sec_upper != "HEADER"
                    && sec_upper != "TEXT"
                    && !sec_upper.contains("HEADER.FIELDS")
                    && section
                        .as_bytes()
                        .first()
                        .is_some_and(|b| b.is_ascii_digit())
                {
                    let s = section.to_string();
                    if !sections.contains(&s) {
                        sections.push(s);
                    }
                }
                search_from = abs_start + end_rel + 1;
            } else {
                break;
            }
        }
    }

    sections
}

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

/// extract header section from raw message (up to \r\n\r\n)
pub fn extract_header_section(data: &[u8]) -> Vec<u8> {
    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
        data[..pos + 4].to_vec()
    } else if let Some(pos) = data.windows(2).position(|w| w == b"\n\n") {
        data[..pos + 2].to_vec()
    } else {
        data.to_vec()
    }
}

/// extract body section from raw message (after \r\n\r\n)
pub fn extract_body_section(data: &[u8]) -> Vec<u8> {
    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
        data[pos + 4..].to_vec()
    } else if let Some(pos) = data.windows(2).position(|w| w == b"\n\n") {
        data[pos + 2..].to_vec()
    } else {
        Vec::new()
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
                        && pos > part_start {
                            parts.push(&body[part_start..pos]);
                        }
                break;
            }
            if in_parts
                && let Some(pos) = find_line_offset(body, i)
                    && pos > part_start {
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

/// find byte offset of line number in body
pub fn find_line_offset(body: &[u8], target_line: usize) -> Option<usize> {
    let mut line_num = 0;
    let mut pos = 0;
    while pos < body.len() {
        if line_num == target_line {
            return Some(pos);
        }
        if let Some(nl) = body[pos..].iter().position(|&b| b == b'\n') {
            pos = pos + nl + 1;
        } else {
            pos = body.len();
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

/// build a single part's BODYSTRUCTURE string (with extension data)
fn build_part_bodystructure(part_data: &[u8]) -> String {
    let header = extract_header_section(part_data);
    let header_str = String::from_utf8_lossy(&header);
    let body = extract_body_section(part_data);
    let info = parse_mime_headers(&header_str);

    if info.media_type == "MULTIPART" {
        if let Some(ref boundary) = info.boundary {
            let parts = split_mime_parts(&body, boundary);
            let parts_str: String = parts.iter().map(|p| build_part_bodystructure(p)).collect();
            return format!(
                "({} \"{}\" (\"boundary\" \"{}\") NIL NIL)",
                parts_str,
                info.subtype.to_lowercase(),
                boundary,
            );
        }
        let body_trimmed = trim_part_trailing_newline(&body);
        let body_lines = body_trimmed.split(|&b| b == b'\n').count();
        return format!(
            "(\"text\" \"plain\" (\"charset\" \"UTF-8\") NIL NIL \"7bit\" {} {} NIL NIL NIL)",
            body_trimmed.len(),
            body_lines,
        );
    }

    let body_trimmed = trim_part_trailing_newline(&body);

    let params = {
        let mut pairs = Vec::new();
        if info.media_type == "TEXT" {
            pairs.push(format!("\"charset\" \"{}\"", info.charset));
        }
        if let Some(ref n) = info.name {
            pairs.push(format!("\"name\" \"{}\"", escape_imap_str(n)));
        }
        if pairs.is_empty() {
            "NIL".to_string()
        } else {
            format!("({})", pairs.join(" "))
        }
    };

    let cid = info
        .content_id
        .as_ref()
        .map(|id| format!("\"<{}>\"", id))
        .unwrap_or_else(|| "NIL".to_string());

    let dsp = if let Some(ref disp) = info.disposition {
        if let Some(fname) = info.disposition_filename.as_ref().or(info.name.as_ref()) {
            format!(
                "(\"{}\" (\"filename\" \"{}\"))",
                disp,
                escape_imap_str(fname)
            )
        } else {
            format!("(\"{}\" NIL)", disp)
        }
    } else {
        "NIL".to_string()
    };

    if info.media_type == "TEXT" {
        let body_lines = body_trimmed.split(|&b| b == b'\n').count();
        format!(
            "(\"text\" \"{}\" {} {} NIL \"{}\" {} {} NIL {} NIL)",
            info.subtype.to_lowercase(),
            params,
            cid,
            info.encoding.to_lowercase(),
            body_trimmed.len(),
            body_lines,
            dsp,
        )
    } else {
        format!(
            "(\"{}\" \"{}\" {} {} NIL \"{}\" {} NIL {} NIL)",
            info.media_type.to_lowercase(),
            info.subtype.to_lowercase(),
            params,
            cid,
            info.encoding.to_lowercase(),
            body_trimmed.len(),
            dsp,
        )
    }
}

/// escape double quotes in IMAP string literals
pub fn escape_imap_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// build BODYSTRUCTURE for a message (handles multipart, with extension data)
pub fn build_bodystructure(data: &[u8]) -> String {
    let header_bytes = extract_header_section(data);
    let header = String::from_utf8_lossy(&header_bytes);
    let body = extract_body_section(data);
    let info = parse_mime_headers(&header);

    if info.media_type == "MULTIPART"
        && let Some(ref boundary) = info.boundary {
            let parts = split_mime_parts(&body, boundary);
            if !parts.is_empty() {
                let parts_str: String = parts.iter().map(|p| build_part_bodystructure(p)).collect();
                return format!(
                    "({} \"{}\" (\"boundary\" \"{}\") NIL NIL)",
                    parts_str,
                    info.subtype.to_lowercase(),
                    boundary,
                );
            }
        }

    let params = {
        let mut pairs = Vec::new();
        if info.media_type == "TEXT" {
            pairs.push(format!("\"charset\" \"{}\"", info.charset));
        }
        if let Some(ref n) = info.name {
            pairs.push(format!("\"name\" \"{}\"", escape_imap_str(n)));
        }
        if pairs.is_empty() {
            "NIL".to_string()
        } else {
            format!("({})", pairs.join(" "))
        }
    };
    let cid = info
        .content_id
        .as_ref()
        .map(|id| format!("\"<{}>\"", id))
        .unwrap_or_else(|| "NIL".to_string());
    let dsp = if let Some(ref disp) = info.disposition {
        if let Some(fname) = info.disposition_filename.as_ref().or(info.name.as_ref()) {
            format!(
                "(\"{}\" (\"filename\" \"{}\"))",
                disp,
                escape_imap_str(fname)
            )
        } else {
            format!("(\"{}\" NIL)", disp)
        }
    } else {
        "NIL".to_string()
    };

    if info.media_type == "TEXT" {
        let body_lines = body.split(|&b| b == b'\n').count();
        format!(
            "(\"text\" \"{}\" {} {} NIL \"{}\" {} {} NIL {} NIL)",
            info.subtype.to_lowercase(),
            params,
            cid,
            info.encoding.to_lowercase(),
            body.len(),
            body_lines,
            dsp,
        )
    } else {
        format!(
            "(\"{}\" \"{}\" {} {} NIL \"{}\" {} NIL {} NIL)",
            info.media_type.to_lowercase(),
            info.subtype.to_lowercase(),
            params,
            cid,
            info.encoding.to_lowercase(),
            body.len(),
            dsp,
        )
    }
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

/// format a comma-separated list of addresses into IMAP address list
pub fn format_addr_list(addrs: &str) -> String {
    let addrs = addrs.trim();
    if addrs.is_empty() {
        return "NIL".to_string();
    }
    let parts: Vec<String> = addrs
        .split(',')
        .map(|a| {
            let a = a.trim();
            if a.is_empty() {
                return String::new();
            }
            let formatted = format_imap_address(a);
            if formatted == "NIL" {
                return String::new();
            }
            formatted[1..formatted.len() - 1].to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        "NIL".to_string()
    } else {
        format!("({})", parts.join(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_flags_all() {
        let flags = FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT | FLAG_RECENT;
        let s = format_imap_flags(flags);
        assert!(s.contains("\\Seen"));
        assert!(s.contains("\\Answered"));
        assert!(s.contains("\\Flagged"));
        assert!(s.contains("\\Deleted"));
        assert!(s.contains("\\Draft"));
        assert!(s.contains("\\Recent"));
    }

    #[test]
    fn format_flags_empty() {
        assert_eq!(format_imap_flags(0), "");
    }

    #[test]
    fn parse_flags_round_trip() {
        let bits = FLAG_SEEN | FLAG_FLAGGED;
        let s = format_imap_flags(bits);
        let parsed = parse_imap_flags(&format!("({})", s));
        assert_eq!(parsed, bits);
    }

    #[test]
    fn parse_flags_case_insensitive() {
        let bits = parse_imap_flags("(\\seen \\FLAGGED \\Draft)");
        assert_eq!(bits, FLAG_SEEN | FLAG_FLAGGED | FLAG_DRAFT);
    }

    #[test]
    fn quote_or_nil_empty() {
        assert_eq!(quote_or_nil(""), "NIL");
    }

    #[test]
    fn quote_or_nil_value() {
        assert_eq!(quote_or_nil("hello"), "\"hello\"");
    }

    #[test]
    fn quote_or_nil_escapes() {
        let result = quote_or_nil("he said \"hi\"");
        assert!(result.contains("\\\""));
    }

    #[test]
    fn format_imap_address_plain() {
        let result = format_imap_address("user@example.com");
        assert!(result.contains("\"user\""));
        assert!(result.contains("\"example.com\""));
    }

    #[test]
    fn format_imap_address_with_name() {
        let result = format_imap_address("John Doe <john@example.com>");
        assert!(result.contains("\"John Doe\""));
        assert!(result.contains("\"john\""));
        assert!(result.contains("\"example.com\""));
    }

    #[test]
    fn format_imap_address_empty() {
        assert_eq!(format_imap_address(""), "NIL");
    }

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
    fn build_bodystructure_simple_text() {
        let msg = b"Content-Type: text/plain; charset=UTF-8\r\n\r\nHello world";
        let bs = build_bodystructure(msg);
        assert!(bs.contains("\"text\""));
        assert!(bs.contains("\"plain\""));
        assert!(bs.contains("\"charset\" \"UTF-8\""));
    }

    #[test]
    fn format_addr_list_single() {
        let result = format_addr_list("user@example.com");
        assert!(result.contains("\"user\""));
        assert!(result.contains("\"example.com\""));
    }

    #[test]
    fn format_addr_list_multiple() {
        let result = format_addr_list("a@b.com, c@d.com");
        assert!(result.contains("\"a\""));
        assert!(result.contains("\"c\""));
    }

    #[test]
    fn format_addr_list_empty() {
        assert_eq!(format_addr_list(""), "NIL");
    }

    #[test]
    fn parse_header_fields_request_basic() {
        let (fields, _) = parse_header_fields_request("BODY[HEADER.FIELDS (FROM TO SUBJECT)]").unwrap();
        assert_eq!(fields, vec!["FROM", "TO", "SUBJECT"]);
    }

    #[test]
    fn parse_generic_body_sections_basic() {
        let sections = parse_generic_body_sections("BODY[1] BODY.PEEK[2]");
        assert!(sections.contains(&"1".to_string()));
        assert!(sections.contains(&"2".to_string()));
    }

    #[test]
    fn parse_generic_body_sections_ignores_header() {
        let sections = parse_generic_body_sections("BODY[HEADER]");
        assert!(sections.is_empty());
    }

    #[test]
    fn format_internal_date_epoch() {
        let result = format_internal_date(0);
        assert!(result.contains("1970"));
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
}

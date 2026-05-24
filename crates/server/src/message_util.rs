use serde::Serialize;

// rfc2047_encode lives in `mailrs-rfc2047` (1.1.0+) as `encode`.
// rfc2231_encode_param lives in `mailrs-rfc2231` (1.0.0+) as `encode_param`.
// Both thin server-internal wrappers were deleted; call sites use the
// published crates directly via `mailrs_rfc2047::encode` and
// `mailrs_rfc2231::encode_param`.

#[derive(Serialize, Clone)]
pub(crate) struct AttachmentInfo {
    pub filename: String,
    pub content_type: String,
    pub size: u32,
}

pub(crate) fn read_message_raw(
    maildir_root: &str,
    user: &str,
    maildir_id: &str,
) -> Option<Vec<u8>> {
    let (local, domain) = user.split_once('@')?;
    let path = format!("{maildir_root}/{domain}/{local}");
    let md = mailrs_maildir::Maildir::open(&path);

    let find_in = |entries: Vec<mailrs_maildir::Entry>| -> Option<Vec<u8>> {
        entries
            .into_iter()
            .find(|e| e.id.to_string() == maildir_id)
            .and_then(|e| std::fs::read(&e.path).ok())
    };

    find_in(md.scan_cur().unwrap_or_default())
        .or_else(|| find_in(md.scan_new().unwrap_or_default()))
}

/// extract a header value from raw RFC 5322 bytes (handles folded headers)
pub(crate) fn extract_header_from_raw(data: &[u8], name: &str) -> String {
    let text = String::from_utf8_lossy(data);
    let prefix = format!("{name}:");
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.is_empty() {
            break; // end of headers
        }
        if line.len() > prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            let mut value = line[prefix.len()..].trim().to_string();
            // collect continuation lines (start with space or tab)
            while i + 1 < lines.len() {
                let next = lines[i + 1];
                if next.starts_with(' ') || next.starts_with('\t') {
                    value.push(' ');
                    value.push_str(next.trim());
                    i += 1;
                } else {
                    break;
                }
            }
            return value;
        }
        i += 1;
    }
    String::new()
}

/// Parse raw message bytes into text body, html body, and attachment list.
///
/// Now backed by `mailrs-mime` (the workspace-owned MIME tree parser).
/// The shape of the returned tuple is unchanged.
pub(crate) fn parse_message(data: &[u8]) -> (Option<String>, Option<String>, Vec<AttachmentInfo>) {
    let root = mailrs_mime::parse(data);

    // Collect first text/plain and first text/html via depth-first walk.
    let mut text_body: Option<String> = None;
    let mut html_body: Option<String> = None;
    for part in root.walk() {
        let mt = part.content_type.mime_type();
        if text_body.is_none() && mt == "text/plain" {
            text_body = part.body_text();
        } else if html_body.is_none() && mt == "text/html" {
            html_body = part.body_text();
        }
        if text_body.is_some() && html_body.is_some() {
            break;
        }
    }

    // If we ended up with no parts whatsoever (rare malformed input),
    // fall back to treating the raw bytes as plain text.
    if text_body.is_none() && html_body.is_none() && root.children.is_empty() {
        if root.content_type.type_ == "text" {
            text_body = root.body_text();
        } else {
            text_body = Some(String::from_utf8_lossy(data).into_owned());
        }
    }

    // Ensure text_body is always present: derive from html if missing.
    let text_body = text_body.or_else(|| {
        html_body
            .as_deref()
            .and_then(|html| html2text::from_read(html.as_bytes(), 80).ok())
    });

    let attachments = root
        .attachments()
        .map(|att| {
            let filename = att.attachment_filename().unwrap_or("unnamed").to_string();
            let content_type = att.content_type.mime_type();
            let content_type = if content_type.ends_with('/') || content_type.starts_with('/') {
                "application/octet-stream".to_string()
            } else {
                content_type
            };
            AttachmentInfo {
                filename,
                content_type,
                size: att.body.len() as u32,
            }
        })
        .collect();

    (text_body, html_body, attachments)
}

/// Decode RFC 2047 encoded-word in header values stored in the database.
///
/// Now backed directly by `mailrs-rfc2047::decode` — used to go through
/// a fake-message round trip via `mail-parser` just to reach its
/// encoded-word decoder. Now one direct call.
pub(crate) fn decode_header(value: &str) -> String {
    if !value.contains("=?") {
        return value.to_string();
    }
    mailrs_rfc2047::decode(value.as_bytes()).into_owned()
}

#[cfg(test)]
#[path = "message_util_tests.rs"]
mod tests;

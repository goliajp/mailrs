use mail_parser::MimeHeaders;
use serde::Serialize;

#[derive(Serialize, Clone)]
pub(crate) struct AttachmentInfo {
    pub filename: String,
    pub content_type: String,
    pub size: u32,
}

pub(crate) fn read_message_raw(maildir_root: &str, user: &str, maildir_id: &str) -> Option<Vec<u8>> {
    let (local, domain) = user.split_once('@')?;
    let path = format!("{maildir_root}/{domain}/{local}");
    let md = mailrs_storage_maildir::Maildir::open(&path);

    let find_in = |entries: Vec<mailrs_storage_maildir::Entry>| -> Option<Vec<u8>> {
        entries
            .into_iter()
            .find(|e| e.id.to_string() == maildir_id)
            .and_then(|e| std::fs::read(&e.path).ok())
    };

    find_in(md.scan_cur().unwrap_or_default())
        .or_else(|| find_in(md.scan_new().unwrap_or_default()))
}

/// extract a header value from raw RFC 5322 bytes
pub(crate) fn extract_header_from_raw(data: &[u8], name: &str) -> String {
    let text = String::from_utf8_lossy(data);
    let prefix = format!("{name}:");
    for line in text.lines() {
        if line.len() > prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            return line[prefix.len()..].trim().to_string();
        }
        if line.is_empty() {
            break;
        }
    }
    String::new()
}

/// parse raw message bytes into text body, html body, and attachment list
pub(crate) fn parse_message(data: &[u8]) -> (Option<String>, Option<String>, Vec<AttachmentInfo>) {
    let msg = match mail_parser::MessageParser::default().parse(data) {
        Some(m) => m,
        None => {
            // fallback: treat entire message as plain text
            return (
                Some(String::from_utf8_lossy(data).into_owned()),
                None,
                vec![],
            );
        }
    };

    let text_body = msg.body_text(0).map(|s| s.into_owned());
    let html_body = msg.body_html(0).map(|s| s.into_owned());

    // ensure text_body is always present: derive from html if missing
    let text_body = text_body.or_else(|| {
        html_body
            .as_deref()
            .and_then(|html| html2text::from_read(html.as_bytes(), 80).ok())
    });

    let attachments = msg
        .attachments()
        .map(|att| {
            let filename = att
                .attachment_name()
                .unwrap_or("unnamed")
                .to_string();
            let content_type = att
                .content_type()
                .map(|ct: &mail_parser::ContentType| {
                    if let Some(sub) = ct.subtype() {
                        format!("{}/{}", ct.ctype(), sub)
                    } else {
                        ct.ctype().to_string()
                    }
                })
                .unwrap_or_else(|| "application/octet-stream".into());
            AttachmentInfo {
                filename,
                content_type,
                size: att.len() as u32,
            }
        })
        .collect();

    (text_body, html_body, attachments)
}

/// decode RFC 2047 encoded-word in header values stored in the database
pub(crate) fn decode_header(value: &str) -> String {
    if !value.contains("=?") {
        return value.to_string();
    }
    let fake = format!("Subject: {value}\r\n\r\n");
    mail_parser::MessageParser::default()
        .parse(fake.as_bytes())
        .and_then(|m| m.subject().map(|s| s.to_string()))
        .unwrap_or_else(|| value.to_string())
}

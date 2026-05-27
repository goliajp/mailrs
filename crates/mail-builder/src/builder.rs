//! `MessageBuilder` — the public face of the crate.

use std::fmt;

use crate::encode::{
    ContentTransferEncoding, choose_cte, encode_base64, encode_quoted_printable, fold_header,
    maybe_encode_word,
};
use crate::multipart::{PartBytes, multipart_envelope};

/// One attachment: filename + content-type + raw bytes. The
/// builder picks the CTE automatically (almost always `base64`) and
/// emits a `Content-Disposition: attachment; filename="..."` header.
#[derive(Debug, Clone)]
pub struct Attachment {
    /// Filename as it should appear in the `Content-Disposition` header.
    pub filename: String,
    /// MIME content-type (e.g. `"application/pdf"`).
    pub content_type: String,
    /// Raw bytes (will be base64-encoded into the message).
    pub data: Vec<u8>,
}

impl Attachment {
    /// Construct an attachment.
    pub fn new(filename: impl Into<String>, content_type: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self {
            filename: filename.into(),
            content_type: content_type.into(),
            data: data.into(),
        }
    }
}

/// Builder for outbound RFC 5322 messages.
///
/// Construction is chain-style: `MessageBuilder::new().from(..).to(..).subject(..)`.
/// Call [`build`](Self::build) for the raw bytes or use the
/// [`Display`] impl for a UTF-8 string view.
#[derive(Debug, Clone, Default)]
pub struct MessageBuilder {
    from: Option<Address>,
    reply_to: Option<Address>,
    to: Vec<Address>,
    cc: Vec<Address>,
    bcc: Vec<Address>,
    subject: Option<String>,
    date: Option<String>,
    message_id: Option<String>,
    text_body: Option<String>,
    html_body: Option<String>,
    attachments: Vec<Attachment>,
    extra_headers: Vec<(String, String)>,
}

/// Single mailbox address with optional display name. Internal-only;
/// not part of the public API — pass addresses to setters as `&str`.
#[derive(Debug, Clone)]
struct Address {
    display: Option<String>,
    email: String,
}

impl Address {
    fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        // Try "Display Name <email@host>" form first.
        if let Some(open) = trimmed.rfind('<')
            && trimmed.ends_with('>')
        {
            let display = trimmed[..open].trim().trim_matches('"').to_string();
            let email = trimmed[open + 1..trimmed.len() - 1].trim().to_string();
            return Self {
                display: if display.is_empty() { None } else { Some(display) },
                email,
            };
        }
        // Bare address form.
        Self {
            display: None,
            email: trimmed.to_string(),
        }
    }

    fn render(&self) -> String {
        match &self.display {
            None => self.email.clone(),
            Some(d) => {
                let encoded = maybe_encode_word(d);
                let needs_quotes = !d.is_ascii() || d.contains([',', ';', '<', '>', '@', '"']);
                if needs_quotes && d.is_ascii() {
                    format!("\"{d}\" <{}>", self.email)
                } else if encoded == d.as_str() {
                    format!("{d} <{}>", self.email)
                } else {
                    // encoded-word; encoded-words may NOT appear
                    // inside quoted-string, so emit raw.
                    format!("{encoded} <{}>", self.email)
                }
            }
        }
    }
}

impl MessageBuilder {
    /// Empty builder. All fields default-empty.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set `From:` (single mailbox).
    pub fn from(mut self, addr: impl AsRef<str>) -> Self {
        self.from = Some(Address::parse(addr.as_ref()));
        self
    }

    /// Set `Reply-To:` (single mailbox).
    pub fn reply_to(mut self, addr: impl AsRef<str>) -> Self {
        self.reply_to = Some(Address::parse(addr.as_ref()));
        self
    }

    /// Append one `To:` recipient.
    pub fn to(mut self, addr: impl AsRef<str>) -> Self {
        self.to.push(Address::parse(addr.as_ref()));
        self
    }

    /// Append one `Cc:` recipient.
    pub fn cc(mut self, addr: impl AsRef<str>) -> Self {
        self.cc.push(Address::parse(addr.as_ref()));
        self
    }

    /// Append one `Bcc:` recipient. Bcc is emitted into the message
    /// body so a downstream MTA strips it before relay; callers
    /// emitting via the SMTP envelope should NOT add the bcc here.
    pub fn bcc(mut self, addr: impl AsRef<str>) -> Self {
        self.bcc.push(Address::parse(addr.as_ref()));
        self
    }

    /// Set `Subject:`. Non-ASCII values are RFC 2047 encoded.
    pub fn subject(mut self, s: impl Into<String>) -> Self {
        self.subject = Some(s.into());
        self
    }

    /// Set `Date:`. If omitted, `build()` fills in the current UTC
    /// in RFC 5322 §3.3 format.
    pub fn date(mut self, s: impl Into<String>) -> Self {
        self.date = Some(s.into());
        self
    }

    /// Set `Message-ID:`. Must include the angle brackets
    /// (e.g. `<abc@example.com>`).
    pub fn message_id(mut self, s: impl Into<String>) -> Self {
        self.message_id = Some(s.into());
        self
    }

    /// Set the text/plain body.
    pub fn text_body(mut self, s: impl Into<String>) -> Self {
        self.text_body = Some(s.into());
        self
    }

    /// Set the text/html body. If both `text_body` and `html_body`
    /// are set, the message becomes `multipart/alternative`.
    pub fn html_body(mut self, s: impl Into<String>) -> Self {
        self.html_body = Some(s.into());
        self
    }

    /// Append an attachment. Adding any attachment promotes the
    /// message to `multipart/mixed`.
    pub fn attachment(mut self, att: Attachment) -> Self {
        self.attachments.push(att);
        self
    }

    /// Add an arbitrary extra header. Use sparingly — almost every
    /// standard header has a typed setter above. Header values are
    /// folded and encoded-word-protected automatically.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }

    /// Render the message to raw bytes.
    pub fn build(&self) -> Vec<u8> {
        let mut out = Vec::new();

        // === headers ===
        if let Some(f) = &self.from {
            push_header(&mut out, "From", &f.render());
        }
        if let Some(rt) = &self.reply_to {
            push_header(&mut out, "Reply-To", &rt.render());
        }
        if !self.to.is_empty() {
            push_header(&mut out, "To", &render_address_list(&self.to));
        }
        if !self.cc.is_empty() {
            push_header(&mut out, "Cc", &render_address_list(&self.cc));
        }
        if !self.bcc.is_empty() {
            push_header(&mut out, "Bcc", &render_address_list(&self.bcc));
        }
        if let Some(s) = &self.subject {
            push_header(&mut out, "Subject", &maybe_encode_word(s));
        }
        let date_str = match &self.date {
            Some(d) => d.clone(),
            None => chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S +0000").to_string(),
        };
        push_header(&mut out, "Date", &date_str);
        if let Some(mid) = &self.message_id {
            push_header(&mut out, "Message-ID", mid);
        }
        for (name, value) in &self.extra_headers {
            let encoded = maybe_encode_word(value);
            push_header(&mut out, name, &encoded);
        }
        push_header(&mut out, "MIME-Version", "1.0");

        // === body structure ===
        let has_attachments = !self.attachments.is_empty();
        let has_alternative = self.text_body.is_some() && self.html_body.is_some();

        let _ = has_alternative; // multipart-mixed handler re-checks both bodies internally
        if has_attachments {
            self.render_multipart_mixed(&mut out);
        } else if has_alternative {
            self.render_multipart_alternative(&mut out);
        } else {
            self.render_singlepart(&mut out);
        }

        out
    }

    fn render_singlepart(&self, out: &mut Vec<u8>) {
        let (body_bytes, ct) = if let Some(html) = &self.html_body {
            (html.as_bytes().to_vec(), "text/html; charset=utf-8")
        } else {
            let text = self.text_body.as_deref().unwrap_or("");
            (text.as_bytes().to_vec(), "text/plain; charset=utf-8")
        };
        let cte = choose_cte(&body_bytes);
        push_header(out, "Content-Type", ct);
        push_header(out, "Content-Transfer-Encoding", cte.as_str());
        out.extend_from_slice(b"\r\n");
        write_encoded_body(out, &body_bytes, cte);
    }

    fn render_multipart_alternative(&self, out: &mut Vec<u8>) {
        let text_part = self.text_body.as_deref().unwrap_or("").as_bytes().to_vec();
        let html_part = self.html_body.as_deref().unwrap_or("").as_bytes().to_vec();
        let parts = vec![text_part_bytes(&text_part), html_part_bytes(&html_part)];
        let (boundary, envelope) = multipart_envelope(&parts);
        push_header(
            out,
            "Content-Type",
            &format!("multipart/alternative; boundary=\"{boundary}\""),
        );
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&envelope);
    }

    fn render_multipart_mixed(&self, out: &mut Vec<u8>) {
        let body_part = if self.text_body.is_some() && self.html_body.is_some() {
            // nest multipart/alternative as the first part of mixed
            let inner_parts = vec![
                text_part_bytes(self.text_body.as_deref().unwrap_or("").as_bytes()),
                html_part_bytes(self.html_body.as_deref().unwrap_or("").as_bytes()),
            ];
            let (inner_boundary, inner_envelope) = multipart_envelope(&inner_parts);
            let mut headers = Vec::new();
            push_header(
                &mut headers,
                "Content-Type",
                &format!("multipart/alternative; boundary=\"{inner_boundary}\""),
            );
            PartBytes {
                headers,
                body: inner_envelope,
            }
        } else if let Some(html) = &self.html_body {
            html_part_bytes(html.as_bytes())
        } else {
            text_part_bytes(self.text_body.as_deref().unwrap_or("").as_bytes())
        };
        let mut parts = vec![body_part];
        for att in &self.attachments {
            parts.push(attachment_part_bytes(att));
        }
        let (boundary, envelope) = multipart_envelope(&parts);
        push_header(
            out,
            "Content-Type",
            &format!("multipart/mixed; boundary=\"{boundary}\""),
        );
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&envelope);
    }
}

impl fmt::Display for MessageBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.build();
        // mail builder output is always ASCII at the wire level
        // (encoded-words + qp/base64 ensure no high bits); a
        // lossless utf8 conversion is the right contract.
        let s = std::str::from_utf8(&bytes).map_err(|_| fmt::Error)?;
        f.write_str(s)
    }
}

fn render_address_list(addrs: &[Address]) -> String {
    addrs.iter().map(Address::render).collect::<Vec<_>>().join(", ")
}

fn text_part_bytes(body: &[u8]) -> PartBytes {
    let cte = choose_cte(body);
    let mut headers = Vec::new();
    push_header(&mut headers, "Content-Type", "text/plain; charset=utf-8");
    push_header(&mut headers, "Content-Transfer-Encoding", cte.as_str());
    let mut body_bytes = Vec::new();
    write_encoded_body(&mut body_bytes, body, cte);
    PartBytes { headers, body: body_bytes }
}

fn html_part_bytes(body: &[u8]) -> PartBytes {
    let cte = choose_cte(body);
    let mut headers = Vec::new();
    push_header(&mut headers, "Content-Type", "text/html; charset=utf-8");
    push_header(&mut headers, "Content-Transfer-Encoding", cte.as_str());
    let mut body_bytes = Vec::new();
    write_encoded_body(&mut body_bytes, body, cte);
    PartBytes { headers, body: body_bytes }
}

fn attachment_part_bytes(att: &Attachment) -> PartBytes {
    // Attachments are emitted as base64 unconditionally — they're
    // almost always binary, and base64 keeps them safely transport-
    // neutral.
    let mut headers = Vec::new();
    push_header(&mut headers, "Content-Type", &att.content_type);
    push_header(&mut headers, "Content-Transfer-Encoding", "base64");
    push_header(
        &mut headers,
        "Content-Disposition",
        &format!("attachment; filename=\"{}\"", att.filename.replace('"', "")),
    );
    let body = encode_base64(&att.data).into_bytes();
    PartBytes { headers, body }
}

fn push_header(out: &mut Vec<u8>, name: &str, value: &str) {
    let line = fold_header(name, value);
    out.extend_from_slice(line.as_bytes());
    out.extend_from_slice(b"\r\n");
}

fn write_encoded_body(out: &mut Vec<u8>, body: &[u8], cte: ContentTransferEncoding) {
    match cte {
        ContentTransferEncoding::SevenBit | ContentTransferEncoding::EightBit => {
            out.extend_from_slice(body);
            if !body.ends_with(b"\r\n") && !body.is_empty() {
                out.extend_from_slice(b"\r\n");
            }
        }
        ContentTransferEncoding::QuotedPrintable => {
            out.extend_from_slice(encode_quoted_printable(body).as_bytes());
            if !body.is_empty() {
                out.extend_from_slice(b"\r\n");
            }
        }
        ContentTransferEncoding::Base64 => {
            out.extend_from_slice(encode_base64(body).as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_parse_bare_email() {
        let a = Address::parse("alice@example.com");
        assert_eq!(a.email, "alice@example.com");
        assert!(a.display.is_none());
    }

    #[test]
    fn address_parse_display_form() {
        let a = Address::parse("Alice <alice@example.com>");
        assert_eq!(a.email, "alice@example.com");
        assert_eq!(a.display.as_deref(), Some("Alice"));
    }

    #[test]
    fn address_parse_quoted_display() {
        let a = Address::parse("\"Alice, the Great\" <alice@example.com>");
        assert_eq!(a.email, "alice@example.com");
        assert_eq!(a.display.as_deref(), Some("Alice, the Great"));
    }

    #[test]
    fn render_bare_address() {
        let a = Address::parse("alice@example.com");
        assert_eq!(a.render(), "alice@example.com");
    }

    #[test]
    fn render_display_ascii_no_special() {
        let a = Address::parse("Alice <alice@example.com>");
        assert_eq!(a.render(), "Alice <alice@example.com>");
    }

    #[test]
    fn render_display_ascii_with_comma_gets_quoted() {
        let a = Address::parse("Alice, Sr. <alice@example.com>");
        // parse trims at the < boundary; the comma sits in the display half
        assert!(a.render().contains("\""));
    }

    #[test]
    fn build_minimal_plain_text() {
        let msg = MessageBuilder::new()
            .from("alice@example.com")
            .to("bob@example.com")
            .subject("hi")
            .text_body("hello")
            .date("Wed, 27 May 2026 12:00:00 +0000")
            .message_id("<m1@example.com>")
            .build();
        let s = std::str::from_utf8(&msg).unwrap();
        assert!(s.contains("From: alice@example.com\r\n"));
        assert!(s.contains("To: bob@example.com\r\n"));
        assert!(s.contains("Subject: hi\r\n"));
        assert!(s.contains("Date: Wed, 27 May 2026 12:00:00 +0000\r\n"));
        assert!(s.contains("Message-ID: <m1@example.com>\r\n"));
        assert!(s.contains("MIME-Version: 1.0\r\n"));
        assert!(s.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(s.contains("Content-Transfer-Encoding: 7bit\r\n"));
        assert!(s.contains("\r\n\r\nhello\r\n"));
    }

    #[test]
    fn build_subject_non_ascii_uses_encoded_word() {
        let msg = MessageBuilder::new()
            .from("alice@example.com")
            .to("bob@example.com")
            .subject("こんにちは")
            .text_body("hello")
            .build();
        let s = std::str::from_utf8(&msg).unwrap();
        let subj_line = s.lines().find(|l| l.starts_with("Subject: ")).unwrap();
        assert!(subj_line.contains("=?UTF-8?"));
    }

    #[test]
    fn build_default_date_is_present() {
        let msg = MessageBuilder::new()
            .from("a@x")
            .to("b@y")
            .subject("s")
            .text_body("hi")
            .build();
        let s = std::str::from_utf8(&msg).unwrap();
        assert!(s.contains("\r\nDate: "));
    }

    #[test]
    fn build_high_bit_body_uses_qp() {
        let msg = MessageBuilder::new()
            .from("a@x")
            .to("b@y")
            .subject("s")
            .text_body("héllo")
            .date("Wed, 27 May 2026 12:00:00 +0000")
            .build();
        let s = std::str::from_utf8(&msg).unwrap();
        assert!(s.contains("Content-Transfer-Encoding: quoted-printable\r\n"));
        assert!(s.contains("h=C3=A9llo"));
    }

    #[test]
    fn build_text_plus_html_is_multipart_alternative() {
        let msg = MessageBuilder::new()
            .from("a@x")
            .to("b@y")
            .subject("s")
            .text_body("hello")
            .html_body("<p>hello</p>")
            .date("Wed, 27 May 2026 12:00:00 +0000")
            .build();
        let s = std::str::from_utf8(&msg).unwrap();
        assert!(s.contains("Content-Type: multipart/alternative;"));
        assert!(s.contains("text/plain"));
        assert!(s.contains("text/html"));
        assert!(s.contains("hello"));
        assert!(s.contains("<p>hello</p>"));
    }

    #[test]
    fn build_with_attachment_is_multipart_mixed() {
        let msg = MessageBuilder::new()
            .from("a@x")
            .to("b@y")
            .subject("s")
            .text_body("hello")
            .attachment(Attachment::new("doc.pdf", "application/pdf", vec![0xFF, 0xD8, 0xFF, 0xE0]))
            .date("Wed, 27 May 2026 12:00:00 +0000")
            .build();
        let s = std::str::from_utf8(&msg).unwrap();
        assert!(s.contains("Content-Type: multipart/mixed;"));
        assert!(s.contains("application/pdf"));
        assert!(s.contains("Content-Disposition: attachment; filename=\"doc.pdf\""));
        assert!(s.contains("Content-Transfer-Encoding: base64\r\n"));
    }

    #[test]
    fn display_matches_build() {
        let mb = MessageBuilder::new()
            .from("a@x")
            .to("b@y")
            .subject("s")
            .text_body("hi")
            .date("Wed, 27 May 2026 12:00:00 +0000");
        let from_display = format!("{mb}");
        let from_build = std::str::from_utf8(&mb.build()).unwrap().to_string();
        assert_eq!(from_display, from_build);
    }

    #[test]
    fn build_with_cc_bcc() {
        let msg = MessageBuilder::new()
            .from("a@x")
            .to("b@y")
            .cc("c@y")
            .bcc("d@y")
            .subject("s")
            .text_body("hi")
            .date("Wed, 27 May 2026 12:00:00 +0000")
            .build();
        let s = std::str::from_utf8(&msg).unwrap();
        assert!(s.contains("To: b@y\r\n"));
        assert!(s.contains("Cc: c@y\r\n"));
        assert!(s.contains("Bcc: d@y\r\n"));
    }

    #[test]
    fn build_extra_header() {
        let msg = MessageBuilder::new()
            .from("a@x")
            .to("b@y")
            .subject("s")
            .text_body("hi")
            .header("X-Mailer", "mailrs-mail-builder")
            .date("Wed, 27 May 2026 12:00:00 +0000")
            .build();
        let s = std::str::from_utf8(&msg).unwrap();
        assert!(s.contains("X-Mailer: mailrs-mail-builder\r\n"));
    }
}

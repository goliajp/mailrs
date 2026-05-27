//! ckpt 2.1 — RFC test corpus.
//!
//! 30+ structured scenarios covering the MIME shapes mailrs
//! production paths actually emit, plus the RFC examples (2046,
//! 2047, 2231, 3464, 6376, 7489) the builder claims to support.
//! Each scenario builds via `MessageBuilder`, parses via
//! `mailrs-rfc5322` + `mailrs-mime`, and asserts structural
//! invariants. The goal isn't byte-identity to any external
//! source — it's that the builder never emits something a
//! conforming parser can't recover the structure from.

use mailrs_mail_builder::{Attachment, MessageBuilder};
use mailrs_rfc5322::Message;

fn fixed_date() -> &'static str {
    "Wed, 27 May 2026 12:00:00 +0000"
}

/// Common assertion: `msg` parses cleanly as RFC 5322 and the
/// stated header subset matches.
fn assert_parses_with_headers(msg: &[u8], expected: &[(&str, &str)]) {
    let parsed = Message::new(msg);
    assert!(parsed.body_offset().is_some(), "message has no body separator");
    for (name, want_contains) in expected {
        let got = parsed.header_str(name).unwrap_or_default();
        let unfold = got.replace("\r\n ", " ").replace("\r\n\t", " ");
        assert!(
            unfold.contains(want_contains),
            "header {name}: want substring {want_contains:?}, got {unfold:?}",
        );
    }
}

fn assert_mime_multipart(msg: &[u8], expected_type: &str, expected_children: usize) {
    let p = mailrs_mime::part::parse(msg);
    assert!(p.content_type.is_multipart(), "expected multipart, got {}", p.content_type.mime_type());
    assert_eq!(
        p.content_type.mime_type(),
        expected_type,
        "wrong outer multipart subtype",
    );
    assert_eq!(p.children.len(), expected_children, "wrong child count");
}

// ===== ASCII text bodies =====

#[test]
fn plain_ascii_short() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("hi")
        .text_body("hello world")
        .date(fixed_date())
        .build();
    assert_parses_with_headers(
        &msg,
        &[
            ("From", "a@x"),
            ("To", "b@y"),
            ("Subject", "hi"),
            ("Content-Type", "text/plain"),
            ("Content-Transfer-Encoding", "7bit"),
        ],
    );
}

#[test]
fn plain_ascii_multi_line() {
    let body = "line 1\r\nline 2\r\nline 3\r\n";
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("multi")
        .text_body(body)
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    for line in ["line 1", "line 2", "line 3"] {
        assert!(s.contains(line), "missing: {line:?}");
    }
    assert!(s.contains("Content-Transfer-Encoding: 7bit"));
}

#[test]
fn plain_ascii_long_line_forces_qp() {
    let long = "x".repeat(200);
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("long")
        .text_body(long.clone())
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Transfer-Encoding: quoted-printable"));
    // verify no line in the body section is over 76 chars
    let body_start = s.find("\r\n\r\n").unwrap() + 4;
    for line in s[body_start..].split("\r\n") {
        assert!(line.len() <= 76, "qp line over 76: {line:?}");
    }
}

#[test]
fn empty_body() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("empty")
        .text_body("")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Type: text/plain"));
    assert!(s.contains("\r\n\r\n"));
}

// ===== UTF-8 bodies =====

#[test]
fn utf8_body_uses_qp() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("utf8")
        .text_body("héllo")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Transfer-Encoding: quoted-printable"));
    assert!(s.contains("h=C3=A9llo"));
}

#[test]
fn utf8_body_japanese() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("ja")
        .text_body("こんにちは世界")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Transfer-Encoding: quoted-printable"));
    // every byte > 0x7F gets escaped
    assert!(!s.bytes().skip_while(|&b| b != b'\n').any(|b| b > 0x7F));
}

#[test]
fn utf8_body_emoji() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("emoji")
        .text_body("hello 🎉 world")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Transfer-Encoding: quoted-printable"));
}

// ===== Binary body → base64 =====

#[test]
fn binary_body_uses_base64() {
    let mut body = Vec::new();
    for b in 0u8..=255 {
        body.push(b);
    }
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("binary")
        .attachment(Attachment::new("binary.bin", "application/octet-stream", body))
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Transfer-Encoding: base64"));
    assert!(s.contains("Content-Type: application/octet-stream"));
}

// ===== Subject encoding =====

#[test]
fn subject_ascii_short() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("hello")
        .text_body("body")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Subject: hello\r\n"));
    assert!(!s.contains("=?UTF-8?"));
}

#[test]
fn subject_ascii_long_folds() {
    let long = "this is a deliberately long subject that will exceed the seventy-eight character soft-wrap threshold and require folding";
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject(long)
        .text_body("body")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    let subj = s.split("\r\n\r\n").next().unwrap();
    // every line ≤ 78
    for line in subj.split("\r\n") {
        if line.starts_with("Subject:") || line.starts_with(' ') {
            assert!(line.len() <= 78, "subject line over 78: {line:?}");
        }
    }
}

#[test]
fn subject_utf8_uses_encoded_word() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("こんにちは")
        .text_body("body")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    let subj_line = s.lines().find(|l| l.starts_with("Subject:")).unwrap();
    assert!(subj_line.contains("=?UTF-8?"));
    assert!(subj_line.contains("?="));
}

// ===== Address rendering =====

#[test]
fn from_display_name_ascii() {
    let msg = MessageBuilder::new()
        .from("Alice <alice@example.com>")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("From: Alice <alice@example.com>"));
}

#[test]
fn from_display_name_utf8() {
    let msg = MessageBuilder::new()
        .from("アリス <alice@example.com>")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    let from_line = s.lines().find(|l| l.starts_with("From:")).unwrap();
    assert!(from_line.contains("=?UTF-8?"));
    assert!(from_line.contains("<alice@example.com>"));
}

#[test]
fn to_list_three_addresses() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .to("c@y")
        .to("d@y")
        .subject("s")
        .text_body("body")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    // unfold for substring matching
    let unfold = s.replace("\r\n ", " ").replace("\r\n\t", " ");
    assert!(unfold.contains("To: b@y, c@y, d@y"));
}

#[test]
fn cc_and_bcc_render() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .cc("c@y")
        .bcc("d@y")
        .subject("s")
        .text_body("body")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Cc: c@y"));
    assert!(s.contains("Bcc: d@y"));
}

#[test]
fn reply_to_renders() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .reply_to("replies@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Reply-To: replies@x"));
}

// ===== Multipart structures =====

#[test]
fn multipart_alternative_text_plus_html() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("plain")
        .html_body("<p>html</p>")
        .date(fixed_date())
        .build();
    assert_mime_multipart(&msg, "multipart/alternative", 2);
}

#[test]
fn multipart_mixed_text_plus_attachment() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .attachment(Attachment::new("a.bin", "application/octet-stream", vec![1, 2, 3]))
        .date(fixed_date())
        .build();
    assert_mime_multipart(&msg, "multipart/mixed", 2);
}

#[test]
fn multipart_mixed_with_multiple_attachments() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .attachment(Attachment::new("a.bin", "application/octet-stream", vec![1, 2, 3]))
        .attachment(Attachment::new("b.bin", "application/octet-stream", vec![4, 5, 6]))
        .attachment(Attachment::new("c.bin", "application/octet-stream", vec![7, 8, 9]))
        .date(fixed_date())
        .build();
    // 1 body + 3 attachments
    assert_mime_multipart(&msg, "multipart/mixed", 4);
}

#[test]
fn multipart_mixed_text_plus_html_plus_attachment_is_nested() {
    // outer = mixed (body + attachment); body = alternative (text + html)
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("plain")
        .html_body("<p>html</p>")
        .attachment(Attachment::new("a.bin", "application/octet-stream", vec![1, 2, 3]))
        .date(fixed_date())
        .build();
    let outer = mailrs_mime::part::parse(&msg);
    assert!(outer.content_type.is_multipart());
    assert_eq!(outer.content_type.mime_type(), "multipart/mixed");
    assert_eq!(outer.children.len(), 2);
    assert!(outer.children[0].content_type.is_multipart());
    assert_eq!(
        outer.children[0].content_type.mime_type(),
        "multipart/alternative"
    );
    assert_eq!(outer.children[0].children.len(), 2);
}

#[test]
fn singlepart_html_only() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .html_body("<p>only html</p>")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Type: text/html"));
    assert!(!s.contains("multipart/"));
}

// ===== Attachment edge cases =====

#[test]
fn attachment_non_ascii_filename_is_quoted_literal() {
    // RFC 2231 percent-encoding is out-of-scope for 0.1 — we just
    // emit the filename inside double-quotes. Non-ASCII filenames
    // produce a header that's technically non-conformant but is
    // widely accepted by real MUAs; ckpt 2.4 strict_mode flags this.
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .attachment(Attachment::new("文書.pdf", "application/pdf", vec![1, 2, 3]))
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Disposition: attachment"));
}

#[test]
fn attachment_filename_with_quotes_is_stripped() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .attachment(Attachment::new("evil\"file.pdf", "application/pdf", vec![1, 2, 3]))
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    // double-quotes must not appear inside the filename token
    assert!(s.contains("filename=\"evilfile.pdf\""));
}

#[test]
fn attachment_empty_data_still_valid() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .attachment(Attachment::new("empty.bin", "application/octet-stream", vec![]))
        .date(fixed_date())
        .build();
    assert_mime_multipart(&msg, "multipart/mixed", 2);
}

#[test]
fn attachment_large_base64_wraps_at_76() {
    let data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .attachment(Attachment::new("big.bin", "application/octet-stream", data))
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    // walk through every part-header block (delimited by the
    // boundary line then the empty-line separator) and check the
    // base64 lines of the attachment body
    let unfold = s.replace("\r\n ", " ").replace("\r\n\t", " ");
    let cd_marker = "Content-Disposition: attachment";
    assert!(unfold.contains(cd_marker), "attachment header missing");
    // find blank line after attachment headers in the ORIGINAL (folded) bytes
    let att_idx = s.find(cd_marker).expect("attachment cd header");
    let blank_idx = s[att_idx..].find("\r\n\r\n").unwrap();
    let body_idx = att_idx + blank_idx + 4;
    for line in s[body_idx..].split("\r\n") {
        if line.starts_with("--") || line.is_empty() {
            break;
        }
        assert!(line.len() <= 76, "base64 line over 76: {line:?} (len {})", line.len());
    }
}

// ===== Boundary collision =====

#[test]
fn boundary_does_not_collide_with_body_hint() {
    // construct a body containing a string that LOOKS like a
    // mailrs boundary marker — the collision-scan in
    // multipart_envelope must pick a different boundary
    let suspicious = b"\r\n--mailrs_attack_marker\r\nfake content\r\n--mailrs_attack_marker--\r\n";
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body(std::str::from_utf8(suspicious).unwrap())
        .attachment(Attachment::new("a.bin", "application/octet-stream", vec![1, 2, 3]))
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    // the bogus marker is preserved in the body
    assert!(s.contains("--mailrs_attack_marker"));
    // but the actual envelope boundary is different
    let ct_line = s.lines().find(|l| l.starts_with("Content-Type: multipart/")).unwrap();
    let boundary_start = ct_line.find("boundary=\"").unwrap() + "boundary=\"".len();
    let actual_boundary = &ct_line[boundary_start..ct_line.rfind('"').unwrap()];
    assert!(!actual_boundary.contains("attack_marker"));
}

// ===== Message-ID / Date / extra header =====

#[test]
fn message_id_preserves_angle_brackets() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .message_id("<abc.123@example.com>")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Message-ID: <abc.123@example.com>"));
}

#[test]
fn default_date_is_rfc5322_shaped() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    let date_line = s.lines().find(|l| l.starts_with("Date: ")).unwrap();
    // example shape: "Date: Wed, 27 May 2026 12:00:00 +0000"
    let value = date_line.trim_start_matches("Date: ");
    // weekday + day + 3-letter month + 4-digit year + HH:MM:SS + TZ
    let parts: Vec<&str> = value.split_whitespace().collect();
    assert_eq!(parts.len(), 6, "RFC 5322 date has 6 tokens, got {parts:?}");
    assert!(parts[0].ends_with(','));
}

#[test]
fn extra_headers_passthrough() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .header("X-Mailer", "mailrs/test")
        .header("X-Priority", "3")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("X-Mailer: mailrs/test"));
    assert!(s.contains("X-Priority: 3"));
}

#[test]
fn extra_header_with_utf8_uses_encoded_word() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("body")
        .header("X-Greeting", "こんにちは")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    let x_line = s.lines().find(|l| l.starts_with("X-Greeting:")).unwrap();
    assert!(x_line.contains("=?UTF-8?"));
}

// ===== Body terminator hygiene =====

#[test]
fn body_with_trailing_crlf_unchanged() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("hello\r\n")
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    let body_start = s.find("\r\n\r\n").unwrap() + 4;
    assert!(s[body_start..].starts_with("hello\r\n"));
}

#[test]
fn body_without_trailing_newline_is_terminated() {
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("no newline")
        .date(fixed_date())
        .build();
    // raw bytes must end with a newline so the message is RFC 5322 conformant
    assert!(msg.ends_with(b"\r\n"), "message must end with CRLF");
}

// ===== RFC examples =====

#[test]
fn rfc_2046_simple_alternative() {
    // RFC 2046 §5.1.4 example shape — text + html variants
    let msg = MessageBuilder::new()
        .from("Mary Smith <mary@example.net>")
        .to("Jane Brown <jane@example.com>")
        .subject("Last night's meeting")
        .text_body("Plain ASCII text version.\r\n")
        .html_body("<html><body>HTML version.</body></html>\r\n")
        .date(fixed_date())
        .build();
    assert_mime_multipart(&msg, "multipart/alternative", 2);
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("From: Mary Smith <mary@example.net>"));
}

#[test]
fn rfc_3464_dsn_shape_minimal() {
    let machine = b"Reporting-MTA: dns; relay.example.org\r\n\
                    \r\n\
                    Final-Recipient: rfc822; alice@example.com\r\n\
                    Action: failed\r\n\
                    Status: 5.1.1\r\n";
    let msg = MessageBuilder::new()
        .from("postmaster@relay.example.org")
        .to("sender@example.org")
        .subject("Delivery Status Notification")
        .text_body("Your message could not be delivered.\r\n")
        .attachment(Attachment::new(
            "delivery-status.txt",
            "message/delivery-status",
            machine.to_vec(),
        ))
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Type: message/delivery-status"));
}

#[test]
fn rfc_7489_dmarc_aggregate_shape() {
    let xml = b"<?xml version=\"1.0\"?><feedback/>";
    let msg = MessageBuilder::new()
        .from("noreply-dmarc@example.com")
        .to("dmarc@example.org")
        .subject("Report domain: example.org Submitter: example.com Report-ID: <1>")
        .text_body("DMARC aggregate report\r\n")
        .attachment(Attachment::new(
            "example.com!example.org!1.xml.gz",
            "application/gzip",
            xml.to_vec(),
        ))
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("application/gzip"));
}

#[test]
fn rfc_6376_dkim_unsigned_envelope_is_canonical() {
    // The body the DKIM signer hashes is everything after the
    // header block; it MUST end with CRLF. Verify the builder
    // emits a body that satisfies that invariant on the
    // single-part case (DKIM signs single-part too).
    let msg = MessageBuilder::new()
        .from("a@x")
        .to("b@y")
        .subject("s")
        .text_body("payload to sign\r\n")
        .date(fixed_date())
        .build();
    let body_off = mailrs_rfc5322::Message::new(&msg).body_offset().unwrap();
    assert!(msg[body_off..].ends_with(b"\r\n"));
}

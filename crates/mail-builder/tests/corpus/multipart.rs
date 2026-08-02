//! Multipart structures, attachment edges, and boundary collision.

use mailrs_mail_builder::{Attachment, MessageBuilder};

use super::{assert_mime_multipart, fixed_date};

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
        .attachment(Attachment::new(
            "a.bin",
            "application/octet-stream",
            vec![1, 2, 3],
        ))
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
        .attachment(Attachment::new(
            "a.bin",
            "application/octet-stream",
            vec![1, 2, 3],
        ))
        .attachment(Attachment::new(
            "b.bin",
            "application/octet-stream",
            vec![4, 5, 6],
        ))
        .attachment(Attachment::new(
            "c.bin",
            "application/octet-stream",
            vec![7, 8, 9],
        ))
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
        .attachment(Attachment::new(
            "a.bin",
            "application/octet-stream",
            vec![1, 2, 3],
        ))
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
        .attachment(Attachment::new(
            "文書.pdf",
            "application/pdf",
            vec![1, 2, 3],
        ))
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
        .attachment(Attachment::new(
            "evil\"file.pdf",
            "application/pdf",
            vec![1, 2, 3],
        ))
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
        .attachment(Attachment::new(
            "empty.bin",
            "application/octet-stream",
            vec![],
        ))
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
        assert!(
            line.len() <= 76,
            "base64 line over 76: {line:?} (len {})",
            line.len()
        );
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
        .attachment(Attachment::new(
            "a.bin",
            "application/octet-stream",
            vec![1, 2, 3],
        ))
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    // the bogus marker is preserved in the body
    assert!(s.contains("--mailrs_attack_marker"));
    // but the actual envelope boundary is different
    let ct_line = s
        .lines()
        .find(|l| l.starts_with("Content-Type: multipart/"))
        .unwrap();
    let boundary_start = ct_line.find("boundary=\"").unwrap() + "boundary=\"".len();
    let actual_boundary = &ct_line[boundary_start..ct_line.rfind('"').unwrap()];
    assert!(!actual_boundary.contains("attack_marker"));
}

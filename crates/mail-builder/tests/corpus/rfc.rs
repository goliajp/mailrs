//! Body-terminator hygiene and the RFC examples the builder claims.

use mailrs_mail_builder::{Attachment, MessageBuilder};

use super::{assert_mime_multipart, fixed_date};

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

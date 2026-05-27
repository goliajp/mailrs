//! ckpt 1 trigger: prove the 0.1 API can build the two mailrs
//! internal use cases (DSN-shaped bounce + DMARC aggregate report
//! email). Byte-identical replacement of the existing
//! `outbound-queue::dsn::format_dsn` / `dmarc::format_report_email`
//! is ckpt 3 work — here we only need to show the produced
//! messages parse cleanly and carry the structural pieces those
//! consumers depend on.

use mailrs_mail_builder::{Attachment, MessageBuilder};
use mailrs_rfc5322::Message;

#[test]
fn dsn_shaped_message_builds_and_parses() {
    // RFC 3464 DSN has two main pieces: a human-readable
    // explanation and a `message/delivery-status` block. For 0.1
    // we model the latter as an attachment-shaped part; the
    // outer container is multipart/mixed (the 1.0 release will
    // expose a multipart/report variant — out of scope for this
    // ckpt).
    let dsn_machine = b"Reporting-MTA: dns; mail.example.com\r\n\
                         \r\n\
                         Final-Recipient: rfc822; bob@dest.com\r\n\
                         Action: failed\r\n\
                         Status: 5.0.0\r\n\
                         Diagnostic-Code: smtp; 550 user unknown\r\n";

    let msg = MessageBuilder::new()
        .from("Mail Delivery System <mailer-daemon@mail.example.com>")
        .to("alice@example.com")
        .subject("Delivery Status Notification (Failure)")
        .header("Auto-Submitted", "auto-replied")
        .text_body(
            "Your message to <bob@dest.com> could not be delivered.\r\n\
             \r\n\
             Error: 550 user unknown\r\n",
        )
        .attachment(Attachment::new(
            "delivery-status.txt",
            "message/delivery-status",
            dsn_machine.to_vec(),
        ))
        .date("Wed, 27 May 2026 12:00:00 +0000")
        .message_id("<dsn-1234@mail.example.com>")
        .build();

    let s = std::str::from_utf8(&msg).expect("builder output is utf-8 / ascii-safe");

    // structural sanity
    assert!(s.contains("From: Mail Delivery System "));
    assert!(s.contains("To: alice@example.com"));
    assert!(s.contains("Subject: Delivery Status Notification (Failure)"));
    assert!(s.contains("Auto-Submitted: auto-replied"));
    assert!(s.contains("Content-Type: multipart/mixed;"));
    assert!(s.contains("Content-Type: message/delivery-status"));
    assert!(s.contains("Content-Disposition: attachment; filename=\"delivery-status.txt\""));

    // parses back via the rfc5322 stone
    let parsed = Message::new(&msg);
    assert_eq!(
        parsed.header_str("Subject").unwrap_or(""),
        "Delivery Status Notification (Failure)"
    );
    let raw_to = parsed.header("To").unwrap_or(b"");
    assert!(std::str::from_utf8(raw_to).unwrap().contains("alice@example.com"));
    assert!(parsed.body_offset().is_some(), "body separator present");
}

#[test]
fn dmarc_report_message_builds_and_parses() {
    // mirrors crates/dmarc::format_report_email's structure:
    // - subject carries the report domain + ID
    // - body: short human-readable text
    // - attachment: gzipped XML report
    let report_xml = b"<?xml version=\"1.0\"?><feedback><report_metadata>\
                       <org_name>example.com</org_name></report_metadata></feedback>";
    let attachment_filename = "example.com!golia.jp!2026-05-27!rpt-42.xml.gz";

    let msg = MessageBuilder::new()
        .from("noreply-dmarc@example.com")
        .to("dmarc@golia.jp")
        .subject("Report domain: golia.jp Submitter: example.com Report-ID: <rpt-42>")
        .text_body("DMARC aggregate report for golia.jp (2026-05-27)\r\n")
        .attachment(Attachment::new(
            attachment_filename,
            "application/gzip",
            // for the test we don't actually gzip — base64 of any
            // bytes is fine to exercise the attachment encoding path
            report_xml.to_vec(),
        ))
        .date("Wed, 27 May 2026 12:00:00 +0000")
        .message_id("<rpt-42@example.com>")
        .build();

    let s = std::str::from_utf8(&msg).expect("builder output is utf-8 / ascii-safe");

    // structural sanity matching the consumer's expectations.
    // Header values may be soft-folded onto continuation lines per
    // RFC 5322 §2.2.3 — unfold before substring-matching so a
    // legitimate fold doesn't trip the test.
    let unfolded = s.replace("\r\n ", " ").replace("\r\n\t", " ");
    assert!(unfolded.contains("Subject: Report domain: golia.jp"));
    assert!(unfolded.contains("Content-Type: multipart/mixed;"));
    assert!(unfolded.contains("Content-Type: application/gzip"));
    assert!(unfolded.contains(&format!(
        "Content-Disposition: attachment; filename=\"{attachment_filename}\""
    )));
    assert!(unfolded.contains("Content-Transfer-Encoding: base64"));

    // base64 of the report bytes must appear in the body
    use base64::Engine;
    let expected_b64 = base64::engine::general_purpose::STANDARD.encode(report_xml);
    // base64 is line-wrapped at 76 — check at least the first chunk
    let first_chunk = &expected_b64.as_bytes()[..64.min(expected_b64.len())];
    let needle = std::str::from_utf8(first_chunk).unwrap();
    assert!(s.contains(needle), "base64 prefix of report missing");

    let parsed = Message::new(&msg);
    assert!(parsed
        .header_str("Subject")
        .unwrap_or("")
        .starts_with("Report domain: golia.jp"));
}

#[test]
fn builder_roundtrips_through_mime_parse() {
    let msg = MessageBuilder::new()
        .from("alice@example.com")
        .to("bob@example.com")
        .subject("Test message")
        .text_body("Plain text.")
        .html_body("<p>HTML</p>")
        .date("Wed, 27 May 2026 12:00:00 +0000")
        .build();

    let part = mailrs_mime::part::parse(&msg);
    assert!(part.content_type.is_multipart());
    assert_eq!(part.content_type.mime_type(), "multipart/alternative");

    assert_eq!(part.children.len(), 2, "two children: text + html");
    assert_eq!(part.children[0].content_type.mime_type(), "text/plain");
    assert_eq!(part.children[1].content_type.mime_type(), "text/html");
}

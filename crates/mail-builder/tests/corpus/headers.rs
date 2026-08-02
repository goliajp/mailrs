//! Header rendering: Subject encoding, addresses, Message-ID and Date.

use mailrs_mail_builder::MessageBuilder;

use super::fixed_date;

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

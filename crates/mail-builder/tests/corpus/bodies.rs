//! Bodies: ASCII, UTF-8, and the binary/base64 path.

use mailrs_mail_builder::{Attachment, MessageBuilder};

use super::{assert_parses_with_headers, fixed_date};

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
        .attachment(Attachment::new(
            "binary.bin",
            "application/octet-stream",
            body,
        ))
        .date(fixed_date())
        .build();
    let s = std::str::from_utf8(&msg).unwrap();
    assert!(s.contains("Content-Transfer-Encoding: base64"));
    assert!(s.contains("Content-Type: application/octet-stream"));
}

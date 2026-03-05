use crate::response::{format_ehlo_response, EnhancedCode, Response};

#[test]
fn simple_250() {
    let r = Response::ok();
    assert_eq!(r.format(), "250 2.0.0 OK\r\n");
}

#[test]
fn greeting() {
    let r = Response::greeting("mx.golia.jp");
    assert_eq!(r.format_greeting(), "220 mx.golia.jp ESMTP MailRS\r\n");
}

#[test]
fn ehlo_multiline() {
    let result = format_ehlo_response("mx.golia.jp", &["PIPELINING", "SIZE 52428800"]);
    assert_eq!(
        result,
        "250-mx.golia.jp\r\n250-PIPELINING\r\n250 SIZE 52428800\r\n"
    );
}

#[test]
fn data_intermediate() {
    let r = Response::data_start();
    assert_eq!(
        r.format(),
        "354 Start mail input; end with <CRLF>.<CRLF>\r\n"
    );
}

#[test]
fn error_503() {
    let r = Response::bad_sequence();
    assert_eq!(r.format(), "503 5.5.1 Bad sequence of commands\r\n");
}

#[test]
fn quit_221() {
    let r = Response::quit();
    assert_eq!(r.format(), "221 2.0.0 Bye\r\n");
}

#[test]
fn tls_ready_220() {
    let r = Response::tls_ready();
    assert_eq!(r.code, 220);
    assert!(r.format().contains("Ready to start TLS"));
}

#[test]
fn auth_challenge_334() {
    let r = Response::auth_challenge("VXNlcm5hbWU6");
    assert_eq!(r.code, 334);
    assert!(r.format().contains("VXNlcm5hbWU6"));
}

#[test]
fn auth_ok_235() {
    let r = Response::auth_ok();
    assert_eq!(r.code, 235);
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 2, subject: 7, detail: 0 })
    );
}

#[test]
fn auth_failed_535() {
    let r = Response::auth_failed();
    assert_eq!(r.code, 535);
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 5, subject: 7, detail: 8 })
    );
}

#[test]
fn ehlo_no_capabilities() {
    let empty: &[&str] = &[];
    let result = format_ehlo_response("mx.golia.jp", empty);
    assert_eq!(result, "250 mx.golia.jp\r\n");
}

#[test]
fn ehlo_single_capability() {
    let result = format_ehlo_response("mx.golia.jp", &["PIPELINING"]);
    assert_eq!(result, "250-mx.golia.jp\r\n250 PIPELINING\r\n");
}

#[test]
fn anti_spam_responses() {
    assert_eq!(Response::rate_limited().code, 421);
    assert_eq!(Response::greylisted().code, 450);
    assert_eq!(Response::spf_reject().code, 550);
    assert_eq!(Response::dmarc_reject().code, 550);
    assert_eq!(Response::dnsbl_reject("zen.spamhaus.org").code, 554);
}

#[test]
fn too_large_response_code() {
    let r = Response::too_large();
    assert_eq!(r.code, 552);
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 5, subject: 3, detail: 4 })
    );
}

#[test]
fn data_ok_250() {
    let r = Response::data_ok();
    assert_eq!(r.code, 250);
    assert!(r.message.contains("queued"));
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 2, subject: 0, detail: 0 })
    );
}

#[test]
fn syntax_error_500() {
    let r = Response::syntax_error();
    assert_eq!(r.code, 500);
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 5, subject: 5, detail: 2 })
    );
    assert!(r.message.contains("Syntax error"));
}

#[test]
fn tls_required_530() {
    let r = Response::tls_required();
    assert_eq!(r.code, 530);
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 5, subject: 7, detail: 0 })
    );
}

#[test]
fn mail_ok_250() {
    let r = Response::mail_ok();
    assert_eq!(r.code, 250);
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 2, subject: 1, detail: 0 })
    );
}

#[test]
fn rcpt_ok_250() {
    let r = Response::rcpt_ok();
    assert_eq!(r.code, 250);
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 2, subject: 1, detail: 5 })
    );
}

#[test]
fn vrfy_252() {
    let r = Response::vrfy();
    assert_eq!(r.code, 252);
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 2, subject: 5, detail: 2 })
    );
}

#[test]
fn help_214() {
    let r = Response::help();
    assert_eq!(r.code, 214);
}

#[test]
fn dnsbl_reject_includes_zone() {
    let r = Response::dnsbl_reject("zen.spamhaus.org");
    assert!(r.message.contains("zen.spamhaus.org"));
}

#[test]
fn response_format_no_enhanced_code() {
    // data_start has no enhanced code
    let r = Response::data_start();
    let formatted = r.format();
    assert_eq!(formatted, "354 Start mail input; end with <CRLF>.<CRLF>\r\n");
}

#[test]
fn response_new_custom() {
    let r = Response::new(
        421,
        Some(EnhancedCode { class: 4, subject: 7, detail: 0 }),
        "custom message",
    );
    assert_eq!(r.code, 421);
    assert_eq!(r.message, "custom message");
    let formatted = r.format();
    assert!(formatted.starts_with("421 4.7.0"));
    assert!(formatted.ends_with("\r\n"));
}

#[test]
fn ehlo_ok_enhanced_code() {
    let r = Response::ehlo_ok();
    assert_eq!(r.code, 250);
    assert_eq!(
        r.enhanced,
        Some(EnhancedCode { class: 2, subject: 0, detail: 0 })
    );
}

#[test]
fn format_ehlo_response_many_capabilities() {
    let caps = &["PIPELINING", "STARTTLS", "AUTH PLAIN LOGIN", "SIZE 52428800"];
    let result = format_ehlo_response("mx.example.com", caps);
    assert!(result.starts_with("250-mx.example.com\r\n"));
    assert!(result.ends_with("250 SIZE 52428800\r\n"));
    // each intermediate line uses "250-"
    assert!(result.contains("250-PIPELINING\r\n"));
    assert!(result.contains("250-STARTTLS\r\n"));
}

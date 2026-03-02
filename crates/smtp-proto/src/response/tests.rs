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

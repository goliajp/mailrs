//! EHLO capabilities and the SIZE parameter.

use super::helpers::*;
use crate::command::{Command, Param, ReversePath};
use crate::session::{Event, MAX_MESSAGE_SIZE, MAX_RECIPIENTS, Session, SessionConfig};

#[test]
fn capabilities_no_tls() {
    let s = session_no_tls();
    let caps = s.capabilities();
    assert!(caps.iter().any(|c| c == "PIPELINING"));
    assert!(!caps.iter().any(|c| c.starts_with("STARTTLS")));
    assert!(!caps.iter().any(|c| c.starts_with("AUTH")));
}

#[test]
fn capabilities_tls_available() {
    let s = session();
    let caps = s.capabilities();
    assert!(caps.iter().any(|c| c == "STARTTLS"));
    // auth not advertised before TLS
    assert!(!caps.iter().any(|c| c.starts_with("AUTH")));
}

#[test]
fn capabilities_tls_active() {
    let s = session_tls();
    let caps = s.capabilities();
    // starttls should NOT be advertised once active
    assert!(!caps.iter().any(|c| c == "STARTTLS"));
    // auth SHOULD be advertised after TLS
    assert!(caps.iter().any(|c| c.starts_with("AUTH")));
}

#[test]
fn capabilities_auth_advertised() {
    // when require_tls_for_auth is false, AUTH advertised even without TLS
    let s = Session::new(
        "mx.test.local",
        SessionConfig {
            require_tls_for_auth: false,
            ..config_no_tls()
        },
    );
    let caps = s.capabilities();
    assert!(caps.iter().any(|c| c.starts_with("AUTH")));
}

#[test]
fn size_param_check_rejects_oversized() {
    let mut s = Session::new(
        "mx.test.local",
        SessionConfig {
            max_size: 1000,
            require_tls_for_auth: false,
            ..config_no_tls()
        },
    );
    greeted(&mut s);
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![Param {
            key: "SIZE",
            value: "2000",
        }],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 552));
}

#[test]
fn size_param_within_limit_accepted() {
    let mut s = Session::new(
        "mx.test.local",
        SessionConfig {
            max_size: 5000,
            require_tls_for_auth: false,
            ..config_no_tls()
        },
    );
    greeted(&mut s);
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![Param {
            key: "SIZE",
            value: "3000",
        }],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
}

#[test]
fn capabilities_include_configured_size() {
    let s = Session::new(
        "mx.test.local",
        SessionConfig {
            max_size: 10485760,
            require_tls_for_auth: false,
            ..config_no_tls()
        },
    );
    let caps = s.capabilities();
    assert!(caps.iter().any(|c| c == "SIZE 10485760"));
}

#[test]
fn session_config_default_values() {
    let cfg = SessionConfig::default();
    assert!(!cfg.tls_available);
    assert!(!cfg.tls_active);
    assert!(cfg.require_tls_for_auth);
    assert_eq!(cfg.max_size, MAX_MESSAGE_SIZE);
    assert_eq!(cfg.max_recipients, MAX_RECIPIENTS);
}

#[test]
fn size_param_non_numeric_ignored() {
    // non-numeric SIZE value should not reject
    let mut s = Session::new(
        "mx.test.local",
        SessionConfig {
            max_size: 1000,
            require_tls_for_auth: false,
            ..config_no_tls()
        },
    );
    greeted(&mut s);
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![Param {
            key: "SIZE",
            value: "abc",
        }],
    });
    // non-parseable size is ignored, message accepted
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
}

#[test]
fn size_param_case_insensitive() {
    let mut s = Session::new(
        "mx.test.local",
        SessionConfig {
            max_size: 100,
            require_tls_for_auth: false,
            ..config_no_tls()
        },
    );
    greeted(&mut s);
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![Param {
            key: "size",
            value: "200",
        }],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 552));
}

#[test]
fn capabilities_always_include_base_extensions() {
    let s = session_no_tls();
    let caps = s.capabilities();
    assert!(caps.iter().any(|c| c == "8BITMIME"));
    assert!(caps.iter().any(|c| c == "ENHANCEDSTATUSCODES"));
    assert!(caps.iter().any(|c| c == "SMTPUTF8"));
}

#[test]
fn capabilities_no_auth_when_tls_required_but_inactive() {
    let s = Session::new(
        "mx.test.local",
        SessionConfig {
            tls_available: true,
            tls_active: false,
            require_tls_for_auth: true,
            ..SessionConfig::default()
        },
    );
    let caps = s.capabilities();
    assert!(!caps.iter().any(|c| c.starts_with("AUTH")));
    assert!(caps.iter().any(|c| c == "STARTTLS"));
}

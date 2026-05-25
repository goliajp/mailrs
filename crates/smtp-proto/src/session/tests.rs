use crate::command::{AuthMechanism, Command, ForwardPath, Param, ReversePath};
use crate::session::{Event, MAX_MESSAGE_SIZE, MAX_RECIPIENTS, Session, SessionConfig, State};

fn config() -> SessionConfig {
    SessionConfig {
        tls_available: true,
        tls_active: false,
        require_tls_for_auth: true,
        max_size: MAX_MESSAGE_SIZE,
        max_recipients: MAX_RECIPIENTS,
    }
}

fn config_tls_active() -> SessionConfig {
    SessionConfig {
        tls_active: true,
        ..config()
    }
}

fn config_no_tls() -> SessionConfig {
    SessionConfig {
        tls_available: false,
        tls_active: false,
        require_tls_for_auth: true,
        max_size: MAX_MESSAGE_SIZE,
        max_recipients: MAX_RECIPIENTS,
    }
}

fn session() -> Session {
    Session::new("mx.test.local", config())
}

fn session_tls() -> Session {
    Session::new("mx.test.local", config_tls_active())
}

fn session_no_tls() -> Session {
    Session::new("mx.test.local", config_no_tls())
}

fn greeted(s: &mut Session) {
    s.handle_command(&Command::Ehlo("client.test"));
}

fn mail_from(s: &mut Session) {
    greeted(s);
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![],
    });
}

fn rcpt_to(s: &mut Session) {
    mail_from(s);
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("rcpt@test.com"),
        params: vec![],
    });
}

// --- normal flow ---

#[test]
fn full_session() {
    let mut s = session_no_tls();
    assert!(matches!(s.state, State::Connected));

    let ev = s.handle_command(&Command::Ehlo("client.test"));
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Greeted { .. }));

    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::MailFrom { .. }));

    let ev = s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("rcpt@test.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::RcptTo { .. }));

    let ev = s.handle_command(&Command::Data);
    assert!(matches!(ev, Event::NeedData { .. }));
}

#[test]
fn multi_rcpt() {
    let mut s = session_no_tls();
    mail_from(&mut s);

    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("a@test.com"),
        params: vec![],
    });
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("b@test.com"),
        params: vec![],
    });
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("c@test.com"),
        params: vec![],
    });

    if let State::RcptTo { forward_paths, .. } = &s.state {
        assert_eq!(forward_paths.len(), 3);
    } else {
        panic!("expected RcptTo state");
    }

    let ev = s.handle_command(&Command::Data);
    if let Event::NeedData { forward_paths, .. } = ev {
        assert_eq!(forward_paths.len(), 3);
    } else {
        panic!("expected NeedData event");
    }
}

// --- bad sequence (503) ---

#[test]
fn mail_before_ehlo() {
    let mut s = session();
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("a@b"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

#[test]
fn rcpt_before_mail() {
    let mut s = session_no_tls();
    greeted(&mut s);
    let ev = s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("a@b"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

#[test]
fn data_before_rcpt() {
    let mut s = session_no_tls();
    mail_from(&mut s);
    let ev = s.handle_command(&Command::Data);
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

#[test]
fn data_at_greeted() {
    let mut s = session_no_tls();
    greeted(&mut s);
    let ev = s.handle_command(&Command::Data);
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

#[test]
fn mail_during_mail() {
    let mut s = session_no_tls();
    mail_from(&mut s);
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("x@y"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

// --- RSET behavior ---

#[test]
fn rset_at_greeted() {
    let mut s = session_no_tls();
    greeted(&mut s);
    let ev = s.handle_command(&Command::Rset);
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Greeted { .. }));
}

#[test]
fn rset_at_mail_from() {
    let mut s = session_no_tls();
    mail_from(&mut s);
    let ev = s.handle_command(&Command::Rset);
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Greeted { .. }));
}

#[test]
fn rset_at_rcpt_to() {
    let mut s = session_no_tls();
    rcpt_to(&mut s);
    let ev = s.handle_command(&Command::Rset);
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Greeted { .. }));
}

// --- EHLO resets ---

#[test]
fn ehlo_resets_mail() {
    let mut s = session_no_tls();
    mail_from(&mut s);
    let ev = s.handle_command(&Command::Ehlo("new.client"));
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Greeted { ref domain, .. } if domain == "new.client"));
}

#[test]
fn ehlo_resets_rcpt() {
    let mut s = session_no_tls();
    rcpt_to(&mut s);
    let ev = s.handle_command(&Command::Ehlo("new.client"));
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Greeted { ref domain, .. } if domain == "new.client"));
}

// --- global commands ---

#[test]
fn noop_at_connected() {
    let mut s = session();
    let ev = s.handle_command(&Command::Noop(None));
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Connected));
}

#[test]
fn noop_at_rcpt() {
    let mut s = session_no_tls();
    rcpt_to(&mut s);
    let ev = s.handle_command(&Command::Noop(None));
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::RcptTo { .. }));
}

#[test]
fn quit_at_any() {
    for setup in [
        |_: &mut Session| {},
        |s: &mut Session| greeted(s),
        |s: &mut Session| mail_from(s),
        |s: &mut Session| rcpt_to(s),
    ] {
        let mut s = session_no_tls();
        setup(&mut s);
        let ev = s.handle_command(&Command::Quit);
        assert!(matches!(ev, Event::Shutdown(r) if r.code == 221));
    }
}

#[test]
fn help_at_any() {
    for setup in [
        |_: &mut Session| {},
        |s: &mut Session| greeted(s),
        |s: &mut Session| mail_from(s),
        |s: &mut Session| rcpt_to(s),
    ] {
        let mut s = session_no_tls();
        setup(&mut s);
        let ev = s.handle_command(&Command::Help(None));
        assert!(matches!(ev, Event::Reply(r) if r.code == 214));
    }
}

#[test]
fn vrfy_at_any() {
    for setup in [
        |_: &mut Session| {},
        |s: &mut Session| greeted(s),
        |s: &mut Session| mail_from(s),
        |s: &mut Session| rcpt_to(s),
    ] {
        let mut s = session_no_tls();
        setup(&mut s);
        let ev = s.handle_command(&Command::Vrfy("user"));
        assert!(matches!(ev, Event::Reply(r) if r.code == 252));
    }
}

// --- capabilities ---

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

// --- STARTTLS state machine ---

#[test]
fn starttls_at_greeted() {
    let mut s = session();
    greeted(&mut s);
    let ev = s.handle_command(&Command::StartTls);
    assert!(matches!(ev, Event::StartTls(r) if r.code == 220));
}

#[test]
fn starttls_at_connected_err() {
    let mut s = session();
    let ev = s.handle_command(&Command::StartTls);
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

#[test]
fn starttls_at_mail_from_err() {
    let mut s = session_no_tls();
    mail_from(&mut s);
    // switch to tls_available config
    s.config.tls_available = true;
    let ev = s.handle_command(&Command::StartTls);
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

#[test]
fn starttls_already_active_err() {
    let mut s = session_tls();
    greeted(&mut s);
    let ev = s.handle_command(&Command::StartTls);
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

// --- AUTH state machine ---

#[test]
fn auth_plain_initial_at_greeted() {
    let mut s = session_tls();
    greeted(&mut s);
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Plain,
        initial_response: Some("dGVzdAB0ZXN0AHBhc3M="),
    });
    assert!(matches!(ev, Event::NeedAuth { .. }));
}

#[test]
fn auth_plain_challenge() {
    let mut s = session_tls();
    greeted(&mut s);
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Plain,
        initial_response: None,
    });
    assert!(matches!(ev, Event::AuthChallenge { .. }));
}

#[test]
fn auth_login_at_greeted() {
    let mut s = session_tls();
    greeted(&mut s);
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Login,
        initial_response: None,
    });
    // LOGIN starts with username challenge
    assert!(matches!(ev, Event::AuthChallenge { .. }));
}

#[test]
fn auth_at_connected_err() {
    let mut s = session_tls();
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Plain,
        initial_response: None,
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

#[test]
fn auth_without_tls_err() {
    let mut s = session();
    greeted(&mut s);
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Plain,
        initial_response: None,
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 530));
}

#[test]
fn auth_already_authenticated_err() {
    let mut s = session_tls();
    greeted(&mut s);
    // manually set authenticated state
    s.state = State::Authenticated {
        domain: "client.test".into(),
        username: "user".into(),
    };
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Plain,
        initial_response: None,
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

#[test]
fn mail_from_after_auth() {
    let mut s = session_tls();
    greeted(&mut s);
    s.state = State::Authenticated {
        domain: "client.test".into(),
        username: "user".into(),
    };
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("user@test.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::MailFrom { .. }));
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

// --- SessionConfig::default ---

#[test]
fn session_config_default_values() {
    let cfg = SessionConfig::default();
    assert!(!cfg.tls_available);
    assert!(!cfg.tls_active);
    assert!(cfg.require_tls_for_auth);
    assert_eq!(cfg.max_size, MAX_MESSAGE_SIZE);
    assert_eq!(cfg.max_recipients, MAX_RECIPIENTS);
}

// --- reset_after_tls ---

#[test]
fn reset_after_tls_sets_active() {
    let mut s = session();
    greeted(&mut s);
    s.reset_after_tls();
    assert!(s.config.tls_active);
    assert!(matches!(s.state, State::Connected));
}

#[test]
fn reset_after_tls_clears_mail_state() {
    let mut s = session();
    // simulate state mid-transaction
    s.state = State::MailFrom {
        domain: "client.test".into(),
        username: None,
        reverse_path: "sender@test.com".into(),
        params: vec![],
    };
    s.reset_after_tls();
    assert!(matches!(s.state, State::Connected));
    assert!(s.config.tls_active);
}

// --- set_authenticated ---

#[test]
fn set_authenticated_from_greeted() {
    let mut s = session_no_tls();
    greeted(&mut s);
    s.set_authenticated("alice".into());
    assert!(matches!(
        s.state,
        State::Authenticated { ref username, .. } if username == "alice"
    ));
}

#[test]
fn set_authenticated_preserves_domain() {
    let mut s = session_no_tls();
    s.handle_command(&Command::Ehlo("myhost.test"));
    s.set_authenticated("bob".into());
    assert!(matches!(
        s.state,
        State::Authenticated { ref domain, ref username, .. }
        if domain == "myhost.test" && username == "bob"
    ));
}

// --- RSET at Connected state ---

#[test]
fn rset_at_connected() {
    let mut s = session();
    let ev = s.handle_command(&Command::Rset);
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Connected));
}

// --- RSET restores to Authenticated when username present ---

#[test]
fn rset_at_mail_from_with_auth_restores_authenticated() {
    let mut s = session_tls();
    greeted(&mut s);
    s.set_authenticated("carol".into());
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("carol@test.com"),
        params: vec![],
    });
    assert!(matches!(s.state, State::MailFrom { .. }));
    let ev = s.handle_command(&Command::Rset);
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(
        s.state,
        State::Authenticated { ref username, .. } if username == "carol"
    ));
}

#[test]
fn rset_at_rcpt_to_with_auth_restores_authenticated() {
    let mut s = session_tls();
    greeted(&mut s);
    s.set_authenticated("dave".into());
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("dave@test.com"),
        params: vec![],
    });
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("rcpt@test.com"),
        params: vec![],
    });
    let ev = s.handle_command(&Command::Rset);
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(
        s.state,
        State::Authenticated { ref username, .. } if username == "dave"
    ));
}

// --- Data command restores to Authenticated when username present ---

#[test]
fn data_cmd_restores_to_authenticated_when_user_present() {
    let mut s = session_tls();
    greeted(&mut s);
    s.set_authenticated("eve".into());
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("eve@test.com"),
        params: vec![],
    });
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("rcpt@test.com"),
        params: vec![],
    });
    let ev = s.handle_command(&Command::Data);
    assert!(matches!(ev, Event::NeedData { .. }));
    assert!(matches!(
        s.state,
        State::Authenticated { ref username, .. } if username == "eve"
    ));
}

// --- handle_auth_response: WaitPlainResponse ---

#[test]
fn auth_response_plain_valid() {
    let mut s = session_tls();
    greeted(&mut s);
    // "test\0test\0pass" base64 encoded
    let ev = s.handle_auth_response(
        "dGVzdAB0ZXN0AHBhc3M=",
        &crate::session::AuthStep::WaitPlainResponse,
    );
    assert!(matches!(ev, Event::NeedAuth { ref username, ref password }
        if username == "test" && password == "pass"
    ));
}

#[test]
fn auth_response_plain_invalid_base64() {
    let mut s = session_tls();
    greeted(&mut s);
    let ev = s.handle_auth_response(
        "!!!invalid!!!",
        &crate::session::AuthStep::WaitPlainResponse,
    );
    assert!(matches!(ev, Event::Reply(r) if r.code == 535));
}

// --- handle_auth_response: WaitUsername ---

#[test]
fn auth_response_login_username() {
    let mut s = session_tls();
    greeted(&mut s);
    // "alice" base64 encoded
    let ev = s.handle_auth_response("YWxpY2U=", &crate::session::AuthStep::WaitUsername);
    // should request password next
    assert!(
        matches!(ev, Event::AuthChallenge { step: crate::session::AuthStep::WaitPassword { ref username }, .. }
            if username == "alice"
        )
    );
}

#[test]
fn auth_response_login_username_invalid_base64() {
    let mut s = session_tls();
    greeted(&mut s);
    let ev = s.handle_auth_response("!!!bad!!!", &crate::session::AuthStep::WaitUsername);
    assert!(matches!(ev, Event::Reply(r) if r.code == 535));
}

// --- handle_auth_response: WaitPassword ---

#[test]
fn auth_response_login_password() {
    let mut s = session_tls();
    greeted(&mut s);
    // "secret" base64 encoded = "c2VjcmV0"
    let ev = s.handle_auth_response(
        "c2VjcmV0",
        &crate::session::AuthStep::WaitPassword {
            username: "alice".into(),
        },
    );
    assert!(matches!(ev, Event::NeedAuth { ref username, ref password }
        if username == "alice" && password == "secret"
    ));
}

#[test]
fn auth_response_login_password_invalid_base64() {
    let mut s = session_tls();
    greeted(&mut s);
    let ev = s.handle_auth_response(
        "!!!bad!!!",
        &crate::session::AuthStep::WaitPassword {
            username: "alice".into(),
        },
    );
    assert!(matches!(ev, Event::Reply(r) if r.code == 535));
}

// --- RCPT TO postmaster ---

#[test]
fn rcpt_to_postmaster_path() {
    let mut s = session_no_tls();
    mail_from(&mut s);
    let ev = s.handle_command(&Command::RcptTo {
        path: ForwardPath::Postmaster,
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    if let State::RcptTo { forward_paths, .. } = &s.state {
        assert_eq!(forward_paths[0], "Postmaster");
    } else {
        panic!("expected RcptTo state");
    }
}

// --- AUTH during mail transaction ---

#[test]
fn auth_during_mail_transaction_err() {
    let mut s = session_tls();
    mail_from(&mut s);
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Plain,
        initial_response: None,
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

#[test]
fn auth_during_rcpt_to_err() {
    let mut s = session_tls();
    rcpt_to(&mut s);
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Plain,
        initial_response: None,
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

// --- SIZE param edge cases ---

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

// --- HELO (legacy) flow ---

#[test]
fn helo_then_mail_from() {
    let mut s = session_no_tls();
    let ev = s.handle_command(&Command::Helo("oldclient.test"));
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Greeted { .. }));

    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
}

// --- STARTTLS not available ---

#[test]
fn starttls_not_available_err() {
    let mut s = session_no_tls();
    greeted(&mut s);
    let ev = s.handle_command(&Command::StartTls);
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

// --- EHLO resets auth state ---

#[test]
fn ehlo_resets_authenticated_state() {
    let mut s = session_tls();
    greeted(&mut s);
    s.set_authenticated("frank".into());
    assert!(matches!(s.state, State::Authenticated { .. }));
    let ev = s.handle_command(&Command::Ehlo("new.client"));
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    // EHLO always transitions to Greeted
    assert!(matches!(s.state, State::Greeted { ref domain, .. } if domain == "new.client"));
}

// --- AUTH PLAIN with malformed payload ---

#[test]
fn auth_plain_bad_initial_response() {
    let mut s = session_tls();
    greeted(&mut s);
    // valid base64 but no null separators
    let encoded =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "notanullbyte");
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Plain,
        initial_response: Some(encoded.as_str()),
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 535));
}

// --- Data NeedData content verification ---

#[test]
fn data_provides_correct_envelope() {
    let mut s = session_no_tls();
    greeted(&mut s);
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("from@example.com"),
        params: vec![],
    });
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("to@example.com"),
        params: vec![],
    });
    let ev = s.handle_command(&Command::Data);
    if let Event::NeedData {
        reverse_path,
        forward_paths,
    } = ev
    {
        assert_eq!(reverse_path, "from@example.com");
        assert_eq!(forward_paths, vec!["to@example.com"]);
    } else {
        panic!("expected NeedData");
    }
}

#[test]
fn data_with_null_reverse_path() {
    let mut s = session_no_tls();
    greeted(&mut s);
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Null,
        params: vec![],
    });
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("bounce@example.com"),
        params: vec![],
    });
    let ev = s.handle_command(&Command::Data);
    if let Event::NeedData { reverse_path, .. } = ev {
        // null reverse path becomes empty string
        assert_eq!(reverse_path, "");
    } else {
        panic!("expected NeedData");
    }
}

// --- recipient limit ---

#[test]
fn rcpt_within_limit_accepted() {
    let mut s = Session::new(
        "mx.test.local",
        SessionConfig {
            max_recipients: 3,
            require_tls_for_auth: false,
            ..config_no_tls()
        },
    );
    mail_from(&mut s);
    for addr in &["a@test.com", "b@test.com", "c@test.com"] {
        let ev = s.handle_command(&Command::RcptTo {
            path: ForwardPath::Path(addr),
            params: vec![],
        });
        assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    }
}

#[test]
fn rcpt_over_limit_rejected() {
    let mut s = Session::new(
        "mx.test.local",
        SessionConfig {
            max_recipients: 2,
            require_tls_for_auth: false,
            ..config_no_tls()
        },
    );
    mail_from(&mut s);
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("a@test.com"),
        params: vec![],
    });
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("b@test.com"),
        params: vec![],
    });
    // third recipient should be rejected (limit is 2)
    let ev = s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("c@test.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 452));
    // session remains in RcptTo state with only 2 recipients
    if let State::RcptTo { forward_paths, .. } = &s.state {
        assert_eq!(forward_paths.len(), 2);
    } else {
        panic!("expected RcptTo state");
    }
}

#[test]
fn rcpt_limit_default_is_100() {
    assert_eq!(MAX_RECIPIENTS, 100);
}

#[test]
fn max_message_size_default_is_50mb() {
    assert_eq!(MAX_MESSAGE_SIZE, 52_428_800);
}

// --- pipelining: MAIL FROM + RCPT TO + DATA in sequence ---

#[test]
fn pipelining_mail_rcpt_data() {
    let mut s = session_no_tls();
    greeted(&mut s);

    let cmds: Vec<Command> = vec![
        Command::MailFrom {
            path: ReversePath::Path("sender@test.com"),
            params: vec![],
        },
        Command::RcptTo {
            path: ForwardPath::Path("rcpt@test.com"),
            params: vec![],
        },
        Command::Data,
    ];

    let events: Vec<Event> = cmds.iter().map(|c| s.handle_command(c)).collect();
    assert!(matches!(events[0], Event::Reply(ref r) if r.code == 250));
    assert!(matches!(events[1], Event::Reply(ref r) if r.code == 250));
    assert!(matches!(events[2], Event::NeedData { .. }));
}

// --- pipelining with error: RCPT before MAIL FROM should fail ---

#[test]
fn pipelining_rcpt_before_mail_fails() {
    let mut s = session_no_tls();
    greeted(&mut s);

    let cmds: Vec<Command> = vec![
        Command::RcptTo {
            path: ForwardPath::Path("rcpt@test.com"),
            params: vec![],
        },
        Command::Data,
    ];

    let events: Vec<Event> = cmds.iter().map(|c| s.handle_command(c)).collect();
    assert!(matches!(events[0], Event::Reply(ref r) if r.code == 503));
    assert!(matches!(events[1], Event::Reply(ref r) if r.code == 503));
}

// --- multiple EHLO resets ---

#[test]
fn multiple_ehlo_resets_to_greeted_each_time() {
    let mut s = session_no_tls();
    for d in &["first.test", "second.test", "third.test"] {
        let ev = s.handle_command(&Command::Ehlo(d));
        assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
        assert!(matches!(s.state, State::Greeted { ref domain } if domain.as_str() == *d));
    }
}

// --- EHLO mid-transaction resets all state ---

#[test]
fn ehlo_during_rcpt_to_resets_transaction() {
    let mut s = session_no_tls();
    rcpt_to(&mut s);
    let ev = s.handle_command(&Command::Ehlo("reset.client"));
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
    assert!(matches!(s.state, State::Greeted { ref domain } if domain == "reset.client"));
    // should not be able to issue DATA now
    let ev = s.handle_command(&Command::Data);
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 503));
}

// --- RSET at Authenticated state stays Authenticated ---

#[test]
fn rset_at_authenticated_stays_authenticated() {
    let mut s = session_tls();
    greeted(&mut s);
    s.set_authenticated("user".into());
    let ev = s.handle_command(&Command::Rset);
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
    assert!(matches!(s.state, State::Authenticated { .. }));
}

// --- full authenticated flow: EHLO -> AUTH -> MAIL -> RCPT -> DATA ---

#[test]
fn full_authenticated_mail_flow() {
    let mut s = session_tls();
    let ev = s.handle_command(&Command::Ehlo("client.test"));
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));

    // simulate successful auth
    s.set_authenticated("alice".into());

    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("alice@test.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
    assert!(
        matches!(s.state, State::MailFrom { ref username, .. } if *username == Some("alice".into()))
    );

    let ev = s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("bob@test.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));

    let ev = s.handle_command(&Command::Data);
    if let Event::NeedData {
        reverse_path,
        forward_paths,
    } = ev
    {
        assert_eq!(reverse_path, "alice@test.com");
        assert_eq!(forward_paths, vec!["bob@test.com"]);
    } else {
        panic!("expected NeedData");
    }
    // after DATA, state should return to Authenticated
    assert!(matches!(s.state, State::Authenticated { ref username, .. } if username == "alice"));
}

// --- multiple transactions in same session ---

#[test]
fn multiple_transactions_same_session() {
    let mut s = session_no_tls();
    greeted(&mut s);

    // first transaction
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("a@test.com"),
        params: vec![],
    });
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("b@test.com"),
        params: vec![],
    });
    let ev = s.handle_command(&Command::Data);
    assert!(matches!(ev, Event::NeedData { .. }));
    assert!(matches!(s.state, State::Greeted { .. }));

    // second transaction on same session
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("c@test.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("d@test.com"),
        params: vec![],
    });
    let ev = s.handle_command(&Command::Data);
    if let Event::NeedData {
        reverse_path,
        forward_paths,
    } = ev
    {
        assert_eq!(reverse_path, "c@test.com");
        assert_eq!(forward_paths, vec!["d@test.com"]);
    } else {
        panic!("expected NeedData");
    }
}

// --- NOOP does not alter state during transaction ---

#[test]
fn noop_preserves_mail_from_state() {
    let mut s = session_no_tls();
    mail_from(&mut s);
    let ev = s.handle_command(&Command::Noop(Some("keep alive")));
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
    assert!(matches!(s.state, State::MailFrom { .. }));
}

// --- HELP and VRFY do not alter state during transaction ---

#[test]
fn help_preserves_rcpt_to_state() {
    let mut s = session_no_tls();
    rcpt_to(&mut s);
    let ev = s.handle_command(&Command::Help(Some("DATA")));
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 214));
    assert!(matches!(s.state, State::RcptTo { .. }));
}

#[test]
fn vrfy_preserves_mail_from_state() {
    let mut s = session_no_tls();
    mail_from(&mut s);
    let ev = s.handle_command(&Command::Vrfy("someone"));
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 252));
    assert!(matches!(s.state, State::MailFrom { .. }));
}

// --- DATA before any EHLO should fail ---

#[test]
fn data_at_connected_fails() {
    let mut s = session();
    let ev = s.handle_command(&Command::Data);
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 503));
}

// --- MAIL FROM at Connected state fails ---

#[test]
fn mail_from_at_connected_fails() {
    let mut s = session_no_tls();
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("a@b.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 503));
}

// --- capabilities: 8BITMIME and ENHANCEDSTATUSCODES always present ---

#[test]
fn capabilities_always_include_base_extensions() {
    let s = session_no_tls();
    let caps = s.capabilities();
    assert!(caps.iter().any(|c| c == "8BITMIME"));
    assert!(caps.iter().any(|c| c == "ENHANCEDSTATUSCODES"));
    assert!(caps.iter().any(|c| c == "SMTPUTF8"));
}

// --- capabilities: auth not advertised when require_tls_for_auth and tls not active ---

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

// --- session hostname is preserved ---

#[test]
fn session_hostname_preserved() {
    let s = Session::new("custom.hostname.example", SessionConfig::default());
    assert_eq!(s.hostname, "custom.hostname.example");
}

// --- AUTH LOGIN full flow via handle_auth_response ---

#[test]
fn auth_login_full_flow() {
    let mut s = session_tls();
    greeted(&mut s);

    // step 1: initiate AUTH LOGIN
    let ev = s.handle_command(&Command::Auth {
        mechanism: AuthMechanism::Login,
        initial_response: None,
    });
    let step = match ev {
        Event::AuthChallenge { step, .. } => step,
        _ => panic!("expected AuthChallenge"),
    };
    assert!(matches!(step, crate::session::AuthStep::WaitUsername));

    // step 2: send username (base64 "testuser" = "dGVzdHVzZXI=")
    let ev = s.handle_auth_response("dGVzdHVzZXI=", &step);
    let step2 = match ev {
        Event::AuthChallenge { step, .. } => step,
        _ => panic!("expected AuthChallenge for password"),
    };
    assert!(
        matches!(step2, crate::session::AuthStep::WaitPassword { ref username } if username == "testuser")
    );

    // step 3: send password (base64 "mypass" = "bXlwYXNz")
    let ev = s.handle_auth_response("bXlwYXNz", &step2);
    assert!(matches!(ev, Event::NeedAuth { ref username, ref password }
        if username == "testuser" && password == "mypass"
    ));
}

// --- DATA with multiple recipients preserves all ---

#[test]
fn data_preserves_all_recipients() {
    let mut s = session_no_tls();
    greeted(&mut s);
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![],
    });
    for addr in &["a@t.com", "b@t.com", "c@t.com", "d@t.com"] {
        s.handle_command(&Command::RcptTo {
            path: ForwardPath::Path(addr),
            params: vec![],
        });
    }
    let ev = s.handle_command(&Command::Data);
    if let Event::NeedData { forward_paths, .. } = ev {
        assert_eq!(forward_paths.len(), 4);
        assert_eq!(forward_paths[0], "a@t.com");
        assert_eq!(forward_paths[3], "d@t.com");
    } else {
        panic!("expected NeedData");
    }
}

// --- STARTTLS then re-greet flow ---

#[test]
fn starttls_then_regreet_flow() {
    let mut s = session();
    greeted(&mut s);
    let ev = s.handle_command(&Command::StartTls);
    assert!(matches!(ev, Event::StartTls(_)));
    s.reset_after_tls();
    assert!(matches!(s.state, State::Connected));
    assert!(s.config.tls_active);

    // must re-greet after TLS upgrade
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("a@b"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 503));

    let ev = s.handle_command(&Command::Ehlo("regreet.client"));
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
    assert!(matches!(s.state, State::Greeted { .. }));
}

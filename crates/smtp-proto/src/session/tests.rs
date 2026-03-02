use crate::command::{AuthMechanism, Command, ForwardPath, Param, ReversePath};
use crate::session::{Event, Session, SessionConfig, State};

fn config() -> SessionConfig {
    SessionConfig {
        tls_available: true,
        tls_active: false,
        require_tls_for_auth: true,
        max_size: 52428800,
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
        max_size: 52428800,
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
    let s = Session::new("mx.test.local", SessionConfig {
        tls_available: false,
        tls_active: false,
        require_tls_for_auth: false,
        max_size: 52428800,
    });
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
    let mut s = Session::new("mx.test.local", SessionConfig {
        tls_available: false,
        tls_active: false,
        require_tls_for_auth: false,
        max_size: 1000,
    });
    greeted(&mut s);
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![Param { key: "SIZE", value: "2000" }],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 552));
}

#[test]
fn size_param_within_limit_accepted() {
    let mut s = Session::new("mx.test.local", SessionConfig {
        tls_available: false,
        tls_active: false,
        require_tls_for_auth: false,
        max_size: 5000,
    });
    greeted(&mut s);
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![Param { key: "SIZE", value: "3000" }],
    });
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
}

#[test]
fn capabilities_include_configured_size() {
    let s = Session::new("mx.test.local", SessionConfig {
        tls_available: false,
        tls_active: false,
        require_tls_for_auth: false,
        max_size: 10485760,
    });
    let caps = s.capabilities();
    assert!(caps.iter().any(|c| c == "SIZE 10485760"));
}

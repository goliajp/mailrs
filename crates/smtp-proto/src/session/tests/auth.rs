//! AUTH, STARTTLS, and what each requires of the session state.

use super::helpers::*;
use crate::command::{AuthMechanism, Command, ForwardPath, ReversePath};
use crate::session::{Event, State};

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

#[test]
fn starttls_not_available_err() {
    let mut s = session_no_tls();
    greeted(&mut s);
    let ev = s.handle_command(&Command::StartTls);
    assert!(matches!(ev, Event::Reply(r) if r.code == 503));
}

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

#[test]
fn rset_at_authenticated_stays_authenticated() {
    let mut s = session_tls();
    greeted(&mut s);
    s.set_authenticated("user".into());
    let ev = s.handle_command(&Command::Rset);
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
    assert!(matches!(s.state, State::Authenticated { .. }));
}

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

//! The happy path and the ordering rules: MAIL before RCPT before DATA.

use super::helpers::*;
use crate::command::{Command, ForwardPath, ReversePath};
use crate::session::{Event, MAX_MESSAGE_SIZE, MAX_RECIPIENTS, Session, SessionConfig, State};

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

#[test]
fn multiple_ehlo_resets_to_greeted_each_time() {
    let mut s = session_no_tls();
    for d in &["first.test", "second.test", "third.test"] {
        let ev = s.handle_command(&Command::Ehlo(d));
        assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
        assert!(matches!(s.state, State::Greeted { ref domain } if domain.as_str() == *d));
    }
}

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

#[test]
fn noop_preserves_mail_from_state() {
    let mut s = session_no_tls();
    mail_from(&mut s);
    let ev = s.handle_command(&Command::Noop(Some("keep alive")));
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 250));
    assert!(matches!(s.state, State::MailFrom { .. }));
}

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

#[test]
fn data_at_connected_fails() {
    let mut s = session();
    let ev = s.handle_command(&Command::Data);
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 503));
}

#[test]
fn mail_from_at_connected_fails() {
    let mut s = session_no_tls();
    let ev = s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("a@b.com"),
        params: vec![],
    });
    assert!(matches!(ev, Event::Reply(ref r) if r.code == 503));
}

#[test]
fn session_hostname_preserved() {
    let s = Session::new("custom.hostname.example", SessionConfig::default());
    assert_eq!(s.hostname, "custom.hostname.example");
}

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

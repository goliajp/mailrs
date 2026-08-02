//! RSET and the resets EHLO implies — including what survives one.

use super::helpers::*;
use crate::command::Command;
use crate::session::{Event, State};

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

#[test]
fn rset_at_connected() {
    let mut s = session();
    let ev = s.handle_command(&Command::Rset);
    assert!(matches!(ev, Event::Reply(r) if r.code == 250));
    assert!(matches!(s.state, State::Connected));
}

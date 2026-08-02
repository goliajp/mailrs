//! Commands out of order, and the ones legal in any state.

use super::helpers::*;
use crate::command::{Command, ForwardPath, ReversePath};
use crate::session::{Event, Session, State};

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

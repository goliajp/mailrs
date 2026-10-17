//! `pending_recipient` names exactly the RCPT `handle_command` would take.

use super::helpers::*;
use crate::session::{Session, SessionConfig, State};

fn rcpt(to: &str) -> Command<'_> {
    Command::RcptTo {
        path: ForwardPath::Path(to),
        params: vec![],
    }
}

fn in_transaction(max_recipients: usize) -> Session {
    let mut s = Session::new(
        "mx.test.local",
        SessionConfig {
            max_recipients,
            ..config_no_tls()
        },
    );
    s.handle_command(&Command::Ehlo("client.test"));
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("a@test.com"),
        params: vec![],
    });
    s
}

#[test]
fn a_rcpt_in_a_transaction_is_pending_and_asking_changes_nothing() {
    let s = in_transaction(2);
    assert_eq!(
        s.pending_recipient(&rcpt("b@test.com")).as_deref(),
        Some("b@test.com")
    );
    assert!(matches!(s.state, State::MailFrom { .. }));
}

#[test]
fn nothing_is_pending_out_of_sequence_or_over_the_limit() {
    let mut s = session_no_tls();
    s.handle_command(&Command::Ehlo("client.test"));
    assert_eq!(s.pending_recipient(&rcpt("b@test.com")), None);

    let mut s = in_transaction(1);
    assert_eq!(s.pending_recipient(&Command::Data), None);
    s.handle_command(&rcpt("b@test.com"));
    assert_eq!(s.pending_recipient(&rcpt("c@test.com")), None);
}

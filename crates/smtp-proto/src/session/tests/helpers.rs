//! Shared fixtures for the session tests.

pub(super) use crate::command::{Command, ForwardPath, ReversePath};
use crate::session::{MAX_MESSAGE_SIZE, MAX_RECIPIENTS, Session, SessionConfig};

pub(super) fn config() -> SessionConfig {
    SessionConfig {
        tls_available: true,
        tls_active: false,
        require_tls_for_auth: true,
        max_size: MAX_MESSAGE_SIZE,
        max_recipients: MAX_RECIPIENTS,
    }
}

pub(super) fn config_tls_active() -> SessionConfig {
    SessionConfig {
        tls_active: true,
        ..config()
    }
}

pub(super) fn config_no_tls() -> SessionConfig {
    SessionConfig {
        tls_available: false,
        tls_active: false,
        require_tls_for_auth: true,
        max_size: MAX_MESSAGE_SIZE,
        max_recipients: MAX_RECIPIENTS,
    }
}

pub(super) fn session() -> Session {
    Session::new("mx.test.local", config())
}

pub(super) fn session_tls() -> Session {
    Session::new("mx.test.local", config_tls_active())
}

pub(super) fn session_no_tls() -> Session {
    Session::new("mx.test.local", config_no_tls())
}

pub(super) fn greeted(s: &mut Session) {
    s.handle_command(&Command::Ehlo("client.test"));
}

pub(super) fn mail_from(s: &mut Session) {
    greeted(s);
    s.handle_command(&Command::MailFrom {
        path: ReversePath::Path("sender@test.com"),
        params: vec![],
    });
}

pub(super) fn rcpt_to(s: &mut Session) {
    mail_from(s);
    s.handle_command(&Command::RcptTo {
        path: ForwardPath::Path("rcpt@test.com"),
        params: vec![],
    });
}

// --- normal flow ---

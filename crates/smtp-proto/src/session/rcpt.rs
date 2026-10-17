//! Letting the caller refuse one recipient before the session takes it.

use super::{Session, State};
use crate::command::{Command, ForwardPath};

pub(super) fn forward_path_to_string(path: &ForwardPath) -> String {
    match path {
        ForwardPath::Postmaster => "Postmaster".to_string(),
        ForwardPath::Path(p) => p.to_string(),
    }
}

impl Session {
    /// The forward path `cmd` would add if it were handed to
    /// [`Session::handle_command`] now; `None` when it would add none (not a
    /// RCPT, out of sequence, or over the recipient limit).
    ///
    /// A caller that refuses this recipient writes its own reply and does
    /// not pass the command on. The transaction then stays as it was, which
    /// is what RFC 5321 §3.3 asks of a rejected RCPT.
    pub fn pending_recipient(&self, cmd: &Command) -> Option<String> {
        let Command::RcptTo { path, .. } = cmd else {
            return None;
        };
        match &self.state {
            State::MailFrom { .. } => {}
            State::RcptTo { forward_paths, .. }
                if forward_paths.len() < self.config.max_recipients => {}
            _ => return None,
        }
        Some(forward_path_to_string(path))
    }
}

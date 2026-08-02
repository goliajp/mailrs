//! The session state machine's states and the events that drive it.

//! SMTP session state machine.
//!
//! [`Session`] owns the per-connection state and drives transitions in
//! response to parsed commands. The caller wires in I/O: it reads a command
//! line from the network, calls [`Session::handle_command`], and acts on
//! the resulting [`Event`] (write a reply, open DATA, start TLS, etc.).

use crate::response::Response;

/// Current SMTP transaction state.
///
/// The state machine advances Connected → Greeted (after EHLO/HELO) → (optional
/// Authenticated, after AUTH) → MailFrom → RcptTo → back to Greeted/Authenticated
/// once DATA finishes. RSET returns to Greeted/Authenticated; STARTTLS returns
/// to Connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Initial state after TCP accept, before any EHLO/HELO.
    Connected,
    /// EHLO/HELO succeeded; no auth yet.
    Greeted {
        /// Domain claimed by the client in EHLO/HELO.
        domain: String,
    },
    /// AUTH succeeded; user is identified.
    Authenticated {
        /// Domain claimed by the client in EHLO/HELO.
        domain: String,
        /// Authenticated username.
        username: String,
    },
    /// MAIL FROM accepted; awaiting one or more RCPT TO.
    MailFrom {
        /// Domain from EHLO/HELO.
        domain: String,
        /// Authenticated username, if any (submission session).
        username: Option<String>,
        /// Envelope sender (reverse path).
        reverse_path: String,
        /// ESMTP MAIL parameters as `(name, value)` pairs.
        params: Vec<(String, String)>,
    },
    /// At least one RCPT TO accepted; awaiting more RCPT TO or DATA.
    RcptTo {
        /// Domain from EHLO/HELO.
        domain: String,
        /// Authenticated username, if any.
        username: Option<String>,
        /// Envelope sender (reverse path).
        reverse_path: String,
        /// ESMTP MAIL parameters.
        params: Vec<(String, String)>,
        /// Envelope recipients accepted so far.
        forward_paths: Vec<String>,
    },
}

/// Continuation step for an in-progress SASL AUTH challenge. Used by
/// [`Event::AuthChallenge`] to tell the caller which kind of response to
/// expect on the next line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStep {
    /// Awaiting the base64 PLAIN payload after a bare `AUTH PLAIN`.
    WaitPlainResponse,
    /// Awaiting the base64 username (LOGIN mechanism, first prompt).
    WaitUsername,
    /// Awaiting the base64 password (LOGIN mechanism, second prompt).
    WaitPassword {
        /// Username already collected in the previous LOGIN prompt.
        username: String,
    },
}

/// Action the caller should take after [`Session::handle_command`] returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Write `response.format()` to the wire and continue reading commands.
    Reply(Response),
    /// MAIL FROM + RCPT TO + DATA all accepted — read the message body until
    /// the `.\r\n` terminator, then call back into the session.
    NeedData {
        /// Envelope sender to associate with the message body.
        reverse_path: String,
        /// Envelope recipients to associate with the message body.
        forward_paths: Vec<String>,
    },
    /// Write the response, then close the connection.
    Shutdown(Response),
    /// Write the response, then upgrade the connection to TLS. After the
    /// upgrade, call [`Session::reset_after_tls`].
    StartTls(Response),
    /// Verify credentials externally, then call
    /// [`Session::set_authenticated`] (or write [`Response::auth_failed`]).
    NeedAuth {
        /// Username to verify.
        username: String,
        /// Password to verify (plaintext, since SASL PLAIN/LOGIN deliver it that way).
        password: String,
    },
    /// Write `response.format()` and read one more line, then call
    /// [`Session::handle_auth_response`] with `step`.
    AuthChallenge {
        /// Challenge response to send to the client.
        response: Response,
        /// What kind of client reply to expect next.
        step: AuthStep,
    },
}

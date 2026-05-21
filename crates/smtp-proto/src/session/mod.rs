//! SMTP session state machine.
//!
//! [`Session`] owns the per-connection state and drives transitions in
//! response to parsed commands. The caller wires in I/O: it reads a command
//! line from the network, calls [`Session::handle_command`], and acts on
//! the resulting [`Event`] (write a reply, open DATA, start TLS, etc.).

use crate::auth::{decode_login_response, decode_plain};
use crate::command::{AuthMechanism, Command, ForwardPath};
use crate::response::Response;

/// Default message size limit in bytes (50 MB). Override via
/// [`SessionConfig::max_size`].
pub const MAX_MESSAGE_SIZE: u64 = 52_428_800;

/// Default per-message recipient limit (RFC 5321 minimum is 100). Override
/// via [`SessionConfig::max_recipients`].
pub const MAX_RECIPIENTS: usize = 100;

fn forward_path_to_string(path: &ForwardPath) -> String {
    match path {
        ForwardPath::Postmaster => "Postmaster".to_string(),
        ForwardPath::Path(p) => p.to_string(),
    }
}

/// Per-session policy knobs. Construct with [`SessionConfig::default()`] and
/// override fields as needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    /// `true` if STARTTLS is advertised on this listener.
    pub tls_available: bool,
    /// `true` once the connection has been upgraded to TLS.
    pub tls_active: bool,
    /// `true` if AUTH commands require an active TLS connection.
    pub require_tls_for_auth: bool,
    /// Max message body size advertised via the `SIZE` ESMTP extension.
    pub max_size: u64,
    /// Max recipients per transaction.
    pub max_recipients: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            tls_available: false,
            tls_active: false,
            require_tls_for_auth: true,
            max_size: MAX_MESSAGE_SIZE,
            max_recipients: MAX_RECIPIENTS,
        }
    }
}

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

/// SMTP session: state machine + config + greeting hostname.
///
/// One [`Session`] per accepted TCP connection. Drive it by repeatedly
/// calling [`Session::handle_command`] with parsed commands and acting on
/// the returned [`Event`].
pub struct Session {
    /// Current transaction state. Read-only by convention — mutate via
    /// `handle_*` methods.
    pub state: State,
    /// Server hostname used in the greeting and EHLO responses.
    pub hostname: String,
    /// Session policy.
    pub config: SessionConfig,
}

impl Session {
    /// Build a fresh session in the [`State::Connected`] state.
    pub fn new(hostname: impl Into<String>, config: SessionConfig) -> Self {
        Self {
            state: State::Connected,
            hostname: hostname.into(),
            config,
        }
    }

    /// Build the EHLO capability list based on the current config — STARTTLS
    /// is omitted once TLS is active, AUTH is omitted if TLS is required but
    /// not yet active, and SIZE reflects `config.max_size`.
    pub fn capabilities(&self) -> Vec<String> {
        let mut caps = vec![
            "PIPELINING".to_string(),
            "8BITMIME".to_string(),
            "ENHANCEDSTATUSCODES".to_string(),
            "SMTPUTF8".to_string(),
        ];

        if self.config.tls_available && !self.config.tls_active {
            caps.push("STARTTLS".to_string());
        }

        let auth_ok = self.config.tls_active || !self.config.require_tls_for_auth;
        if auth_ok {
            caps.push("AUTH PLAIN LOGIN".to_string());
        }

        caps.push(format!("SIZE {}", self.config.max_size));
        caps
    }

    /// Reset state after a successful TLS upgrade. Sets `tls_active = true`
    /// and returns the session to [`State::Connected`] so the client must
    /// re-issue EHLO.
    pub fn reset_after_tls(&mut self) {
        self.config.tls_active = true;
        self.state = State::Connected;
    }

    /// Move the session to [`State::Authenticated`] after the caller has
    /// verified credentials returned by [`Event::NeedAuth`].
    pub fn set_authenticated(&mut self, username: String) {
        let domain = self.extract_domain();
        self.state = State::Authenticated { domain, username };
    }

    /// Drive the state machine: apply `cmd` against the current state and
    /// return the [`Event`] the caller should act on.
    pub fn handle_command(&mut self, cmd: &Command) -> Event {
        // global commands: accepted in any state
        match cmd {
            Command::Quit => return Event::Shutdown(Response::quit()),
            Command::Noop(_) => return Event::Reply(Response::ok()),
            Command::Help(_) => return Event::Reply(Response::help()),
            Command::Vrfy(_) => return Event::Reply(Response::vrfy()),
            _ => {}
        }

        // EHLO/HELO: accepted in any state, resets transaction
        match cmd {
            Command::Ehlo(domain) | Command::Helo(domain) => {
                self.state = State::Greeted {
                    domain: domain.to_string(),
                };
                return Event::Reply(Response::ehlo_ok());
            }
            _ => {}
        }

        // RSET: accepted after greeting, resets to Greeted (or Authenticated)
        if matches!(cmd, Command::Rset) {
            return self.handle_rset();
        }

        // STARTTLS
        if matches!(cmd, Command::StartTls) {
            return self.handle_starttls();
        }

        // AUTH
        if let Command::Auth {
            mechanism,
            initial_response,
        } = cmd
        {
            return self.handle_auth(*mechanism, *initial_response);
        }

        // state-dependent commands
        match (&self.state, cmd) {
            (State::Connected, _) => Event::Reply(Response::bad_sequence()),

            // MAIL FROM: accepted at Greeted or Authenticated
            (State::Greeted { .. } | State::Authenticated { .. }, Command::MailFrom { path, params }) => {
                // check SIZE parameter
                if let Some(size_str) = params.iter().find(|p| p.key.eq_ignore_ascii_case("SIZE")).map(|p| p.value)
                    && let Ok(size) = size_str.parse::<u64>()
                        && size > self.config.max_size {
                            return Event::Reply(Response::too_large());
                        }

                let reverse_path = match path {
                    crate::command::ReversePath::Null => String::new(),
                    crate::command::ReversePath::Path(p) => p.to_string(),
                };
                let owned_params: Vec<(String, String)> = params
                    .iter()
                    .map(|p| (p.key.to_string(), p.value.to_string()))
                    .collect();
                let domain = self.extract_domain();
                let username = self.extract_username();
                self.state = State::MailFrom {
                    domain,
                    username,
                    reverse_path,
                    params: owned_params,
                };
                Event::Reply(Response::mail_ok())
            }

            (State::MailFrom { .. }, Command::RcptTo { path, .. }) => {
                let forward = forward_path_to_string(path);
                let domain = self.extract_domain();
                let username = self.extract_username();
                let (reverse_path, params) = self.extract_mail_from();
                self.state = State::RcptTo {
                    domain,
                    username,
                    reverse_path,
                    params,
                    forward_paths: vec![forward],
                };
                Event::Reply(Response::rcpt_ok())
            }

            (State::RcptTo { .. }, Command::RcptTo { path, .. }) => {
                if let State::RcptTo { forward_paths, .. } = &self.state
                    && forward_paths.len() >= self.config.max_recipients {
                        return Event::Reply(Response::too_many_recipients());
                    }
                let forward = forward_path_to_string(path);
                if let State::RcptTo { forward_paths, .. } = &mut self.state {
                    forward_paths.push(forward);
                }
                Event::Reply(Response::rcpt_ok())
            }

            (State::RcptTo { .. }, Command::Data) => {
                let reverse_path = self.extract_reverse_path();
                let forward_paths = self.extract_forward_paths();
                let domain = self.extract_domain();
                let username = self.extract_username();
                match username {
                    Some(u) => self.state = State::Authenticated { domain, username: u },
                    None => self.state = State::Greeted { domain },
                }
                Event::NeedData {
                    reverse_path,
                    forward_paths,
                }
            }

            _ => Event::Reply(Response::bad_sequence()),
        }
    }

    fn handle_rset(&mut self) -> Event {
        match &self.state {
            State::Connected => Event::Reply(Response::ok()),
            State::Greeted { .. } => Event::Reply(Response::ok()),
            State::Authenticated { .. } => Event::Reply(Response::ok()),
            State::MailFrom { .. } | State::RcptTo { .. } => {
                let domain = self.extract_domain();
                let username = self.extract_username();
                match username {
                    Some(u) => self.state = State::Authenticated { domain, username: u },
                    None => self.state = State::Greeted { domain },
                }
                Event::Reply(Response::ok())
            }
        }
    }

    fn handle_starttls(&mut self) -> Event {
        if self.config.tls_active {
            return Event::Reply(Response::bad_sequence());
        }
        if !self.config.tls_available {
            return Event::Reply(Response::bad_sequence());
        }
        match &self.state {
            State::Greeted { .. } => Event::StartTls(Response::tls_ready()),
            _ => Event::Reply(Response::bad_sequence()),
        }
    }

    fn handle_auth(&mut self, mechanism: AuthMechanism, initial_response: Option<&str>) -> Event {
        // must be greeted first
        if matches!(self.state, State::Connected) {
            return Event::Reply(Response::bad_sequence());
        }
        // already authenticated
        if matches!(self.state, State::Authenticated { .. }) {
            return Event::Reply(Response::bad_sequence());
        }
        // must not be in mail transaction
        if matches!(self.state, State::MailFrom { .. } | State::RcptTo { .. }) {
            return Event::Reply(Response::bad_sequence());
        }
        // require TLS if configured
        if self.config.require_tls_for_auth && !self.config.tls_active {
            return Event::Reply(Response::tls_required());
        }

        match mechanism {
            AuthMechanism::Plain => match initial_response {
                Some(data) => match decode_plain(data) {
                    Ok((username, password)) => Event::NeedAuth { username, password },
                    Err(_) => Event::Reply(Response::auth_failed()),
                },
                None => Event::AuthChallenge {
                    response: Response::auth_challenge(""),
                    step: AuthStep::WaitPlainResponse,
                },
            },
            AuthMechanism::Login => Event::AuthChallenge {
                response: Response::auth_challenge("VXNlcm5hbWU6"),
                step: AuthStep::WaitUsername,
            },
        }
    }

    /// Handle a continuation line after [`Event::AuthChallenge`]. `step` is
    /// the same value carried by the prior `AuthChallenge`. Returns the
    /// next [`Event`] — typically another `AuthChallenge` for LOGIN's
    /// two-prompt flow, or a final `NeedAuth` with credentials to verify.
    pub fn handle_auth_response(
        &mut self,
        data: &str,
        step: &AuthStep,
    ) -> Event {
        match step {
            AuthStep::WaitPlainResponse => match decode_plain(data) {
                Ok((username, password)) => Event::NeedAuth { username, password },
                Err(_) => Event::Reply(Response::auth_failed()),
            },
            AuthStep::WaitUsername => match decode_login_response(data) {
                Ok(username) => Event::AuthChallenge {
                    response: Response::auth_challenge("UGFzc3dvcmQ6"),
                    step: AuthStep::WaitPassword { username },
                },
                Err(_) => Event::Reply(Response::auth_failed()),
            },
            AuthStep::WaitPassword { username } => match decode_login_response(data) {
                Ok(password) => Event::NeedAuth {
                    username: username.clone(),
                    password,
                },
                Err(_) => Event::Reply(Response::auth_failed()),
            },
        }
    }

    fn extract_domain(&self) -> String {
        match &self.state {
            State::Connected => String::new(),
            State::Greeted { domain }
            | State::Authenticated { domain, .. }
            | State::MailFrom { domain, .. }
            | State::RcptTo { domain, .. } => domain.clone(),
        }
    }

    fn extract_username(&self) -> Option<String> {
        match &self.state {
            State::Authenticated { username, .. }
            | State::MailFrom { username: Some(username), .. }
            | State::RcptTo { username: Some(username), .. } => Some(username.clone()),
            _ => None,
        }
    }

    fn extract_mail_from(&self) -> (String, Vec<(String, String)>) {
        match &self.state {
            State::MailFrom {
                reverse_path,
                params,
                ..
            } => (reverse_path.clone(), params.clone()),
            _ => (String::new(), vec![]),
        }
    }

    fn extract_reverse_path(&self) -> String {
        match &self.state {
            State::MailFrom { reverse_path, .. }
            | State::RcptTo { reverse_path, .. } => reverse_path.clone(),
            _ => String::new(),
        }
    }

    fn extract_forward_paths(&self) -> Vec<String> {
        match &self.state {
            State::RcptTo { forward_paths, .. } => forward_paths.clone(),
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests;

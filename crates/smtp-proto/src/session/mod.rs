use crate::auth::{decode_login_response, decode_plain};
use crate::command::{AuthMechanism, Command, ForwardPath};
use crate::response::Response;

/// maximum message size in bytes (50 MB)
pub const MAX_MESSAGE_SIZE: u64 = 52_428_800;

/// maximum number of recipients per message (RFC 5321 minimum is 100)
pub const MAX_RECIPIENTS: usize = 100;

fn forward_path_to_string(path: &ForwardPath) -> String {
    match path {
        ForwardPath::Postmaster => "Postmaster".to_string(),
        ForwardPath::Path(p) => p.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    pub tls_available: bool,
    pub tls_active: bool,
    pub require_tls_for_auth: bool,
    pub max_size: u64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Connected,
    Greeted {
        domain: String,
    },
    Authenticated {
        domain: String,
        username: String,
    },
    MailFrom {
        domain: String,
        username: Option<String>,
        reverse_path: String,
        params: Vec<(String, String)>,
    },
    RcptTo {
        domain: String,
        username: Option<String>,
        reverse_path: String,
        params: Vec<(String, String)>,
        forward_paths: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStep {
    WaitPlainResponse,
    WaitUsername,
    WaitPassword { username: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Reply(Response),
    NeedData {
        reverse_path: String,
        forward_paths: Vec<String>,
    },
    Shutdown(Response),
    StartTls(Response),
    NeedAuth {
        username: String,
        password: String,
    },
    AuthChallenge {
        response: Response,
        step: AuthStep,
    },
}

pub struct Session {
    pub state: State,
    pub hostname: String,
    pub config: SessionConfig,
}

impl Session {
    pub fn new(hostname: impl Into<String>, config: SessionConfig) -> Self {
        Self {
            state: State::Connected,
            hostname: hostname.into(),
            config,
        }
    }

    /// build EHLO capability list based on current config state
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

    /// reset session after successful TLS upgrade
    pub fn reset_after_tls(&mut self) {
        self.config.tls_active = true;
        self.state = State::Connected;
    }

    /// set session to authenticated state after successful auth verification
    pub fn set_authenticated(&mut self, username: String) {
        let domain = self.extract_domain();
        self.state = State::Authenticated { domain, username };
    }

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
                if let Some(size_str) = params.iter().find(|p| p.key.eq_ignore_ascii_case("SIZE")).map(|p| p.value) {
                    if let Ok(size) = size_str.parse::<u64>() {
                        if size > self.config.max_size {
                            return Event::Reply(Response::too_large());
                        }
                    }
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
                if let State::RcptTo { forward_paths, .. } = &self.state {
                    if forward_paths.len() >= self.config.max_recipients {
                        return Event::Reply(Response::too_many_recipients());
                    }
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

    /// handle an AUTH continuation line (base64-encoded response)
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

use std::sync::Arc;

use mailrs_imap_proto::{
    ImapCommand, TaggedCommand, format_bad, format_no, format_ok, parse_command,
};
use mailrs_mailbox::{Mailbox, PgMailboxStore};

use crate::domain_store::DomainStore;
use crate::inbound::auth_guard::AuthGuard;
use crate::users::UserStore;

// IMAP session — split across submodules in this directory:
//   auth, mailbox, fetch, store, search, copy_move, append, uid,
//   idle, quota. Each submodule attaches `impl ImapSession`
//   blocks for its verb group. mod.rs owns the types, the
//   constructor, and the `handle_line` / `handle_command` dispatch.

mod append;
mod auth;
mod copy_move;
mod fetch;
mod idle;
mod mailbox;
mod quota;
mod search;
mod store;
mod uid;

#[cfg(test)]
mod tests;

/// Convert string responses to byte vectors — IMAP wire-format
/// helpers in this module return `Vec<String>` for command results
/// that don't embed binary payloads (most of them); the dispatch
/// layer wraps them into `HandleResult::Responses(Vec<Vec<u8>>)`.
pub(super) fn strs_to_bytes(strs: Vec<String>) -> Vec<Vec<u8>> {
    strs.into_iter().map(|s| s.into_bytes()).collect()
}

/// Result of handling one IMAP command line.
pub enum HandleResult {
    /// Plain response payloads (one wire frame each).
    Responses(Vec<Vec<u8>>),
    /// APPEND-style continuation: client should send `size` more
    /// bytes of literal data, which the session consumes via
    /// [`ImapSession::handle_literal_data`].
    NeedLiteral {
        /// The `+ Ready for literal data` continuation line.
        continuation: Vec<u8>,
        /// Number of literal bytes to read next.
        size: u32,
    },
    /// IDLE-style sustained continuation: the connection enters
    /// idle mode and waits for status updates (driven by the
    /// session manager). The `tag` belongs to the IDLE command
    /// itself so the eventual `DONE` can ack it.
    EnterIdle {
        /// The `+ idling` continuation line.
        continuation: Vec<u8>,
        /// IDLE command tag — replayed on the closing OK.
        tag: String,
    },
}

/// In-flight APPEND state — set by `append::handle_append_start`
/// and consumed by `ImapSession::handle_literal_data`.
pub(super) struct PendingAppend {
    pub(super) tag: String,
    pub(super) mailbox: String,
    pub(super) flags: u32,
}

/// IMAP session state machine — drives an authenticated client
/// connection through the IMAP4rev1 protocol. Each command goes
/// through [`Self::handle_line`] → `handle_command` and dispatches
/// to a per-verb handler in one of the sibling submodules.
/// Field visibility is `pub(super)` so those split-impl handlers
/// can access session state — this is internal plumbing, not part
/// of the public API.
pub struct ImapSession {
    /// PG-backed mailbox / message metadata store.
    pub mailbox_store: Arc<PgMailboxStore>,
    pub(super) users: Arc<UserStore>,
    pub(super) state: ImapState,
    pub(super) pending_append: Option<PendingAppend>,
    /// Filesystem root for Maildir storage (one subdir per
    /// `<domain>/<localpart>`). Used by `read_message_file` and
    /// the APPEND path.
    pub maildir_root: String,
    pub(super) auth_guard: Option<Arc<AuthGuard>>,
    pub(super) peer_addr: Option<std::net::IpAddr>,
    pub(super) domain_store: Option<Arc<DomainStore>>,
    pub(super) ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
}

pub(super) enum ImapState {
    NotAuthenticated,
    Authenticated { username: String },
    Selected { username: String, mailbox: Mailbox },
}

impl ImapSession {
    pub fn new(mailbox_store: Arc<PgMailboxStore>, users: Arc<UserStore>) -> Self {
        Self {
            mailbox_store,
            users,
            state: ImapState::NotAuthenticated,
            pending_append: None,
            maildir_root: String::new(),
            auth_guard: None,
            peer_addr: None,
            domain_store: None,
            ldap_config: None,
        }
    }

    pub fn with_maildir_root(mut self, root: &str) -> Self {
        self.maildir_root = root.to_string();
        self
    }

    pub fn with_auth_guard(mut self, guard: Arc<AuthGuard>, addr: std::net::IpAddr) -> Self {
        self.auth_guard = Some(guard);
        self.peer_addr = Some(addr);
        self
    }

    pub fn with_domain_store(mut self, store: Arc<DomainStore>) -> Self {
        self.domain_store = Some(store);
        self
    }

    pub fn with_ldap_config(mut self, config: Arc<crate::ldap_auth::LdapConfig>) -> Self {
        self.ldap_config = Some(config);
        self
    }

    // ---------- state-borrow helpers used by submodule handlers ----------

    /// Borrow the authenticated username, or produce a tagged
    /// `NO not authenticated` response. Replaces the seven-line
    /// `match &self.state { ... }` boilerplate that every
    /// authenticated-only handler used to start with.
    pub(super) fn authenticated_username(&self, tag: &str) -> Result<&str, Vec<String>> {
        match &self.state {
            ImapState::Authenticated { username } | ImapState::Selected { username, .. } => {
                Ok(username.as_str())
            }
            ImapState::NotAuthenticated => Err(vec![format_no(tag, "not authenticated")]),
        }
    }

    /// Owned-string variant of [`Self::authenticated_username`] —
    /// for handlers that need to move the username into state
    /// (SELECT, EXAMINE) or pass it across an `.await` that
    /// borrows `&mut self`.
    pub(super) fn authenticated_username_owned(&self, tag: &str) -> Result<String, Vec<String>> {
        self.authenticated_username(tag).map(str::to_string)
    }

    /// Borrow the currently-selected mailbox, or produce a tagged
    /// `NO no mailbox selected` response. Replaces the
    /// `match &self.state { Selected{mailbox,..} => mailbox, _ =>
    /// return ... }` pattern at every Selected-only handler entry.
    pub(super) fn selected_mailbox(&self, tag: &str) -> Result<&Mailbox, Vec<String>> {
        match &self.state {
            ImapState::Selected { mailbox, .. } => Ok(mailbox),
            _ => Err(vec![format_no(tag, "no mailbox selected")]),
        }
    }

    /// process a raw command line, return result
    #[tracing::instrument(name = "imap.cmd", skip(self, line), fields(verb))]
    pub async fn handle_line(&mut self, line: &str) -> HandleResult {
        // Record the IMAP verb (first whitespace token after the tag) into
        // the span — gives operators a per-command breakdown in OTel UIs
        // without needing the full line (which can contain passwords).
        if let Some(verb) = line.split_whitespace().nth(1) {
            tracing::Span::current().record("verb", verb.to_ascii_uppercase());
        }
        // log all commands for debugging
        {
            let username = match &self.state {
                ImapState::Selected { username, .. } | ImapState::Authenticated { username } => {
                    username.as_str()
                }
                _ => "?",
            };
            // hide passwords in LOGIN commands. IMAP commands are ASCII;
            // avoid allocating a Unicode-folded copy just to substring-match.
            let redacted = if line.as_bytes().windows(5).any(|w| w.eq_ignore_ascii_case(b"LOGIN"))
            {
                let parts: Vec<&str> = line.splitn(4, ' ').collect();
                if parts.len() >= 4 {
                    format!("{} {} {} ***", parts[0], parts[1], parts[2])
                } else {
                    line.trim().to_string()
                }
            } else {
                line.trim().to_string()
            };
            tracing::debug!(event = "imap_command_recv", user = %username, cmd = %redacted);
        }
        match parse_command(line) {
            Ok(cmd) => self.handle_command(&cmd).await,
            Err(e) => {
                // try to extract tag for error response
                let tag = line.split_whitespace().next().unwrap_or("*");
                HandleResult::Responses(strs_to_bytes(vec![format_bad(
                    tag,
                    &format!("parse error: {e}"),
                )]))
            }
        }
    }

    async fn handle_command(&mut self, cmd: &TaggedCommand) -> HandleResult {
        let tag = &cmd.tag;

        // FETCH returns Vec<Vec<u8>> directly (binary-safe)
        if let ImapCommand::Fetch {
            sequence,
            attributes,
        } = &cmd.command
        {
            return HandleResult::Responses(
                self.handle_fetch(tag, sequence, attributes, false).await,
            );
        }

        // UID might contain FETCH
        if let ImapCommand::Uid { subcommand } = &cmd.command {
            return HandleResult::Responses(self.handle_uid(tag, subcommand.as_ref()).await);
        }

        // all other commands return Vec<String>, convert to Vec<Vec<u8>>
        let responses = match &cmd.command {
            ImapCommand::Capability => self.handle_capability(tag),
            ImapCommand::Login { username, password } => {
                self.handle_login(tag, username, password).await
            }
            ImapCommand::Logout => self.handle_logout(tag),
            ImapCommand::Noop => vec![format_ok(tag, "NOOP completed")],
            ImapCommand::List { reference, pattern } => {
                self.handle_list(tag, reference, pattern).await
            }
            ImapCommand::Select { mailbox } => self.handle_select(tag, mailbox).await,
            ImapCommand::Examine { mailbox } => self.handle_examine(tag, mailbox).await,
            ImapCommand::Store {
                sequence,
                action,
                flags,
            } => self.handle_store(tag, sequence, action, flags, false).await,
            ImapCommand::Search { criteria } => self.handle_search(tag, criteria).await,
            ImapCommand::Expunge => self.handle_expunge(tag).await,
            ImapCommand::Close => self.handle_close(tag).await,
            ImapCommand::Idle => {
                return self.handle_idle(tag);
            }
            ImapCommand::GetQuota { quotaroot } => self.handle_getquota(tag, quotaroot).await,
            ImapCommand::GetQuotaRoot { mailbox } => self.handle_getquotaroot(tag, mailbox).await,
            ImapCommand::Append {
                mailbox,
                flags,
                literal_size,
            } => {
                return self
                    .handle_append_start(tag, mailbox, flags.as_deref(), *literal_size)
                    .await;
            }
            ImapCommand::Copy { sequence, mailbox } => {
                self.handle_copy(tag, sequence, mailbox, false).await
            }
            ImapCommand::Move { sequence, mailbox } => {
                self.handle_move(tag, sequence, mailbox, false).await
            }
            ImapCommand::Status { mailbox, items } => self.handle_status(tag, mailbox, items).await,
            ImapCommand::Create { mailbox } => self.handle_create(tag, mailbox).await,
            ImapCommand::Delete { mailbox } => self.handle_delete(tag, mailbox).await,
            ImapCommand::Rename { from, to } => self.handle_rename(tag, from, to).await,
            ImapCommand::Subscribe { mailbox: _ } => {
                vec![format_ok(tag, "SUBSCRIBE completed")]
            }
            ImapCommand::Unsubscribe { mailbox: _ } => {
                vec![format_ok(tag, "UNSUBSCRIBE completed")]
            }
            ImapCommand::Lsub { reference, pattern } => {
                self.handle_lsub(tag, reference, pattern).await
            }
            ImapCommand::Namespace => self.handle_namespace(tag),
            ImapCommand::Sort {
                criteria,
                search_criteria,
                ..
            } => {
                self.handle_sort(tag, criteria, search_criteria, false)
                    .await
            }
            ImapCommand::Enable(caps) => self.handle_enable(tag, caps),
            ImapCommand::Unselect => self.handle_unselect(tag),
            _ => unreachable!(), // Fetch and Uid handled above
        };
        HandleResult::Responses(strs_to_bytes(responses))
    }
}

/// Format the IMAP server greeting `* OK [<hostname>] IMAP4rev1
/// server ready\r\n` — written by the listener immediately after
/// `accept` so the client knows the server is alive.
pub fn imap_greeting(hostname: &str) -> Vec<u8> {
    format!("* OK [{hostname}] IMAP4rev1 server ready\r\n").into_bytes()
}

/// Drive a single IMAP connection from accept to close. Generic
/// over the underlying stream (`AsyncRead + AsyncWrite`) so the
/// same body serves both plain (`TcpStream`) and TLS-wrapped
/// (`tokio_rustls::server::TlsStream<TcpStream>`) connections.
///
/// Wires the `mailrs-imap-codec` framing layer to the session
/// state machine: each `ImapInput::Line` dispatches into
/// `handle_line`; on `HandleResult::NeedLiteral` it tells the
/// codec to expect a literal of the given size; on
/// `HandleResult::EnterIdle` it nests a `tokio::select!` loop
/// that forwards `SmtpEvent::NewMessage` updates as `*EXISTS`
/// untagged responses until the client sends `DONE`.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(
    name = "imap.conn",
    skip(stream, mailbox_store, users, auth_guard, domain_store, ldap_config, event_bus, hostname, maildir_root),
    fields(peer = %addr),
)]
pub async fn handle_connection<S>(
    stream: S,
    addr: std::net::SocketAddr,
    mailbox_store: Arc<mailrs_mailbox::PgMailboxStore>,
    users: Arc<crate::users::UserStore>,
    auth_guard: Arc<AuthGuard>,
    domain_store: Option<Arc<DomainStore>>,
    ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
    event_bus: crate::event_bus::EventBus,
    hostname: &str,
    maildir_root: &str,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::{SinkExt, StreamExt};
    use tokio_util::codec::Framed;

    metrics::counter!("mailrs_imap_connections_total").increment(1);
    let mut framed = Framed::new(stream, mailrs_imap_codec::ImapCodec::new());
    let greeting = imap_greeting(hostname);
    if framed.send(greeting).await.is_err() {
        return;
    }

    let mut session = ImapSession::new(mailbox_store, users)
        .with_maildir_root(maildir_root)
        .with_auth_guard(auth_guard, addr.ip());
    if let Some(ds) = domain_store {
        session = session.with_domain_store(ds);
    }
    if let Some(ldap) = ldap_config {
        session = session.with_ldap_config(ldap);
    }

    while let Some(result) = framed.next().await {
        match result {
            Ok(mailrs_imap_codec::ImapInput::Line(line)) => {
                let result = session.handle_line(&line).await;
                match result {
                    HandleResult::Responses(responses) => {
                        let is_logout = responses.iter().any(|r| r.windows(3).any(|w| w == b"BYE"));
                        for resp in responses {
                            if framed.send(resp).await.is_err() {
                                return;
                            }
                        }
                        if is_logout {
                            break;
                        }
                    }
                    HandleResult::NeedLiteral { continuation, size } => {
                        if framed.send(continuation).await.is_err() {
                            return;
                        }
                        framed.codec_mut().expect_literal(size);
                    }
                    HandleResult::EnterIdle { continuation, tag } => {
                        if framed.send(continuation).await.is_err() {
                            return;
                        }

                        let idle_user = session.idle_user().map(|s| s.to_string());
                        let mut rx = event_bus.subscribe();

                        loop {
                            tokio::select! {
                                event = rx.recv() => {
                                    match event {
                                        Ok(env) => {
                                            if let crate::event_bus::SmtpEvent::NewMessage { user, .. } = &env.event
                                                && idle_user.as_deref() == Some(user.as_str()) {
                                                let updates = session.idle_status_update().await;
                                                for u in updates {
                                                    if framed.send(u).await.is_err() {
                                                        return;
                                                    }
                                                }
                                            }
                                        }
                                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                                        _ => {}
                                    }
                                }
                                frame = framed.next() => {
                                    if let Some(Ok(mailrs_imap_codec::ImapInput::Line(done_line))) = frame
                                        && done_line.trim().eq_ignore_ascii_case("DONE") {
                                            let resp = mailrs_imap_proto::format_ok(&tag, "IDLE terminated").into_bytes();
                                            if framed.send(resp).await.is_err() {
                                                return;
                                            }
                                        }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Ok(mailrs_imap_codec::ImapInput::LiteralData(data)) => {
                let responses = session.handle_literal_data(&data).await;
                let is_logout = responses.iter().any(|r| r.windows(3).any(|w| w == b"BYE"));
                for resp in responses {
                    if framed.send(resp).await.is_err() {
                        return;
                    }
                }
                if is_logout {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

//! Per-connection IMAP session — state machine + command dispatch.
//!
//! Structure follows RFC 3501 §3: NotAuthenticated → Authenticated →
//! Selected → Logout. LOGIN flips NotAuthenticated → Authenticated;
//! SELECT / EXAMINE flip Authenticated → Selected; LOGOUT / CLOSE
//! terminate the connection. Unknown / unsupported commands answer
//! `BAD`, unauthenticated commands answer `NO`.
//!
//! The session leans heavily on `mailrs-imap-proto` (parser +
//! formatter) and `mailrs-imap-codec` (framing) so we don't reinvent
//! the wire format. Backend calls go through [`crate::imap::backend`].

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use mailrs_imap_codec::{ImapCodec, ImapInput};
use mailrs_imap_proto::{ImapCommand, format_bad, format_bye, format_no, format_ok, parse_command};
use mailrs_tls_reload::TlsState;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use super::auth;
use super::backend::{self, ImapMessage, MailboxInfo};
use crate::FastcoreState;

use super::fetch::*;
use super::mailbox::*;
use super::query::*;

/// Session state per RFC 3501 §3.
#[derive(Debug, Clone)]
pub(super) enum State {
    NotAuthed,
    Authed {
        user: String,
    },
    Selected {
        user: String,
        mailbox: MailboxInfo,
        messages: Vec<ImapMessage>,
        read_only: bool,
    },
}

impl State {
    pub(super) fn user(&self) -> Option<&str> {
        match self {
            State::NotAuthed => None,
            State::Authed { user } | State::Selected { user, .. } => Some(user),
        }
    }
}

/// A connection that is already encrypted, or a plaintext one when no
/// certificate is configured: logins are accepted and nothing offers
/// STARTTLS.
pub async fn run<S>(state: Arc<FastcoreState>, io: S)
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let mut framed = Framed::new(io, ImapCodec::new());
    if greet(&state, &mut framed, false).await {
        serve(&state, &mut framed, false).await;
    }
}

/// A plaintext connection offering STARTTLS. After the upgrade the
/// session starts over on the TLS stream: no greeting (RFC 9051
/// §6.2.1), and nothing the client sent in clear is kept.
pub async fn run_upgradable<S>(state: Arc<FastcoreState>, io: S, tls: TlsState)
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let mut framed = Framed::new(io, ImapCodec::new());
    if !greet(&state, &mut framed, true).await || !serve(&state, &mut framed, true).await {
        return;
    }
    // into_inner drops whatever the client pipelined after STARTTLS
    match tls.acceptor().accept(framed.into_inner()).await {
        Ok(stream) => {
            serve(&state, &mut Framed::new(stream, ImapCodec::new()), false).await;
        }
        Err(e) => tracing::debug!(error = %e, "imap: STARTTLS handshake failed"),
    }
}

/// Greeting per RFC 3501 §7.5. Sent before the client says anything.
async fn greet<S>(state: &FastcoreState, framed: &mut Framed<S, ImapCodec>, starttls: bool) -> bool
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let host = state
        .mailbox
        .store_ref()
        .get(b"hostname")
        .ok()
        .flatten()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| "mailrs".into());
    let caps = auth::capabilities(starttls).join(" ");
    let greeting = format!("* OK [CAPABILITY {caps}] {host} ready\r\n");
    framed.send(greeting.into_bytes()).await.is_ok()
}

/// Runs commands until the connection ends (false) or the client is told
/// to begin TLS (true, only when `starttls`).
async fn serve<S>(
    state: &Arc<FastcoreState>,
    framed: &mut Framed<S, ImapCodec>,
    starttls: bool,
) -> bool
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let mut session = State::NotAuthed;
    // ENABLE QRESYNC is connection-scoped (RFC 7162 §3.2.5)
    let mut qresync = false;
    while let Some(frame) = framed.next().await {
        let Ok(input) = frame else { return false };
        let line = match input {
            ImapInput::Line(s) => s,
            ImapInput::LiteralData(_) => {
                // Standalone literal outside APPEND flow — ignore.
                continue;
            }
        };
        let line = line.trim_end();
        let tag = line.split_once(' ').map_or("", |(t, _)| t).to_string();
        let command = match auth::early(line, &tag, starttls, framed).await {
            auth::Early::Parse => match parse_command(line) {
                Ok(cmd) => cmd.command,
                Err(e) => {
                    let _ = framed
                        .send(format_bad("*", &format!("parse: {e}")).into_bytes())
                        .await;
                    continue;
                }
            },
            auth::Early::Login { username, password } => ImapCommand::Login { username, password },
            auth::Early::Reply(out) => {
                if send_all(framed, out).await {
                    continue;
                }
                return false;
            }
            auth::Early::StartTls => {
                let ok = format_ok(&tag, "Begin TLS negotiation now");
                return framed.send(ok.into_bytes()).await.is_ok();
            }
            auth::Early::Closed => return false,
        };
        let is_logout = matches!(command, ImapCommand::Logout);
        let responses = dispatch(state, &mut session, &tag, command, framed, &mut qresync).await;
        if !send_all(framed, responses).await || is_logout {
            return false;
        }
    }
    false
}

async fn send_all<S>(framed: &mut Framed<S, ImapCodec>, out: Vec<String>) -> bool
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    for r in out {
        if framed.send(r.into_bytes()).await.is_err() {
            return false;
        }
    }
    true
}

async fn dispatch<S>(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    cmd: ImapCommand,
    framed: &mut Framed<S, ImapCodec>,
    qresync: &mut bool,
) -> Vec<String>
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    if let Some(out) = super::mock::intercept(state, session, tag, &cmd, framed).await {
        return out;
    }
    match cmd {
        ImapCommand::Noop => vec![format_ok(tag, "NOOP completed")],
        ImapCommand::Logout => vec![
            format_bye("mailrs logging out"),
            format_ok(tag, "LOGOUT completed"),
        ],
        ImapCommand::Login { username, password } => {
            login(state, session, tag, &username, &password)
        }
        ImapCommand::List {
            reference: _,
            pattern,
        } => list_response(state, session, tag, &pattern),
        ImapCommand::Select { mailbox } => select(state, session, tag, &mailbox, false, *qresync),
        ImapCommand::Examine { mailbox } => select(state, session, tag, &mailbox, true, *qresync),
        ImapCommand::Enable(caps) => {
            let mut enabled = Vec::new();
            for c in &caps {
                let up = c.to_ascii_uppercase();
                if up == "QRESYNC" || up == "CONDSTORE" {
                    if up == "QRESYNC" {
                        *qresync = true;
                    }
                    enabled.push(up);
                }
            }
            vec![
                format!("* ENABLED {}\r\n", enabled.join(" ")),
                format_ok(tag, "ENABLE completed"),
            ]
        }
        ImapCommand::Close => close(session, tag),
        ImapCommand::Fetch {
            sequence,
            attributes,
        } => fetch_response(session, tag, &sequence, &attributes, false),
        ImapCommand::Uid { subcommand } => match *subcommand {
            ImapCommand::Fetch {
                sequence,
                attributes,
            } => fetch_response(session, tag, &sequence, &attributes, true),
            ImapCommand::Store {
                sequence,
                action,
                flags,
            } => store_response(state, session, tag, &sequence, &action, &flags, true),
            ImapCommand::Search { criteria } => search_response(session, tag, &criteria, true),
            ImapCommand::Sort {
                criteria,
                charset: _,
                search_criteria,
            } => sort_response(session, tag, &criteria, &search_criteria, true),
            ImapCommand::Copy { sequence, mailbox } => {
                copy_response(state, session, tag, &sequence, &mailbox, false, true)
            }
            ImapCommand::Move { sequence, mailbox } => {
                copy_response(state, session, tag, &sequence, &mailbox, true, true)
            }
            _ => vec![format_bad(tag, "UID subcommand not supported")],
        },
        ImapCommand::Store {
            sequence,
            action,
            flags,
        } => store_response(state, session, tag, &sequence, &action, &flags, false),
        ImapCommand::Search { criteria } => search_response(session, tag, &criteria, false),
        ImapCommand::Expunge => expunge(state, session, tag, *qresync),
        ImapCommand::Copy { sequence, mailbox } => {
            copy_response(state, session, tag, &sequence, &mailbox, false, false)
        }
        ImapCommand::Move { sequence, mailbox } => {
            copy_response(state, session, tag, &sequence, &mailbox, true, false)
        }
        ImapCommand::Append {
            mailbox,
            flags: _flags,
            literal_size,
        } => append_flow(state, session, tag, framed, &mailbox, literal_size).await,
        ImapCommand::Idle => idle_flow(state, session, tag, framed).await,
        ImapCommand::Sort {
            criteria,
            charset: _,
            search_criteria,
        } => sort_response(session, tag, &criteria, &search_criteria, false),
        ImapCommand::GetQuota { quotaroot: _ } => quota_response(state, session, tag),
        ImapCommand::GetQuotaRoot { mailbox } => {
            let mut out = vec![format!("* QUOTAROOT \"{mailbox}\" \"\"\r\n")];
            out.extend(quota_response(state, session, tag));
            out
        }
        _ => vec![format_bad(tag, "command not implemented")],
    }
}

fn login(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    username: &str,
    password: &str,
) -> Vec<String> {
    if !matches!(session, State::NotAuthed) {
        return vec![format_bad(tag, "already authenticated")];
    }
    if backend::verify_password(state, username, password) {
        *session = State::Authed {
            user: username.to_string(),
        };
        vec![format_ok(tag, "LOGIN completed")]
    } else {
        vec![format_no(tag, "invalid credentials")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_maildir::Flag;

    use crate::imap::fetch::{flags_to_imap, parse_imap_flags};

    #[test]
    fn wildcard_matches_star() {
        assert!(match_wildcard("INBOX", "*"));
        assert!(match_wildcard("Sent", "*"));
        assert!(match_wildcard("Work.Client", "*"));
    }

    #[test]
    fn wildcard_matches_percent() {
        // Maildir has no hierarchy separator in our IMAP naming, so
        // % and * both collapse to "anything" per RFC 3501 §6.3.8.
        assert!(match_wildcard("INBOX", "%"));
        assert!(match_wildcard("Work.Client", "%"));
    }

    #[test]
    fn wildcard_matches_prefix() {
        assert!(match_wildcard("Sent", "S*"));
        assert!(!match_wildcard("Draft", "S*"));
    }

    #[test]
    fn parse_flags_reads_backslash_names() {
        let f = parse_imap_flags("(\\Seen \\Flagged)");
        assert!(f.contains(&Flag::Seen));
        assert!(f.contains(&Flag::Flagged));
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn flags_to_imap_serialises_known() {
        let s = flags_to_imap(&[Flag::Seen, Flag::Flagged]);
        assert_eq!(s, "\\Seen \\Flagged");
    }
}

//! Per-connection IMAP session — state machine + command dispatch.
//!
//! Structure follows RFC 3501 §3: NotAuthenticated → Authenticated →
//! Selected → Logout. LOGIN flips NotAuthenticated → Authenticated;
//! SELECT / EXAMINE flip Authenticated → Selected; LOGOUT ends the
//! connection. Unknown / unsupported commands answer `BAD`,
//! unauthenticated commands answer `NO`.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use mailrs_imap_codec::{ImapCodec, ImapInput};
use mailrs_imap_proto::{ImapCommand, format_bad, format_bye, format_no, format_ok, parse_command};
use mailrs_tls_reload::TlsState;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use super::auth;
use super::fetch::fetch_response;
use super::mailbox::{append_flow, close, idle_flow, list_response, select};
use super::query::{copy_response, quota_response, search_response, sort_response};
use super::store::{expunge, store_response};
use crate::backend::{Account, Directory, Error, Message, Protocol};

pub(super) type Io<S> = Framed<S, ImapCodec>;

/// Session state per RFC 3501 §3.
pub(super) enum State<A: Account> {
    NotAuthed,
    Authed {
        account: Arc<A>,
    },
    Selected {
        account: Arc<A>,
        mailbox: A::Mailbox,
        messages: Vec<Message<A::Key>>,
        read_only: bool,
    },
}

impl<A: Account> State<A> {
    pub(super) fn account(&self) -> Option<&Arc<A>> {
        match self {
            State::NotAuthed => None,
            State::Authed { account } | State::Selected { account, .. } => Some(account),
        }
    }
}

/// `NO` for a failed backend call: a refusal as the store worded it,
/// anything else as `<verb> failed: <error>`, logged.
pub(super) fn no(tag: &str, verb: &str, e: &Error) -> String {
    match e {
        Error::Refused(text) => format_no(tag, text),
        e => {
            tracing::warn!(err = %e, "imap: {verb} failed");
            format_no(tag, &format!("{verb} failed: {e}"))
        }
    }
}

/// A connection that is already encrypted, or a plaintext one when no
/// certificate is configured: logins are accepted and nothing offers
/// STARTTLS.
pub async fn run<D, S>(dir: Arc<D>, io: S)
where
    D: Directory,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let mut framed = Framed::new(io, ImapCodec::new());
    if greet(&*dir, &mut framed, false).await {
        serve(&*dir, &mut framed, false).await;
    }
}

/// A plaintext connection offering STARTTLS. After the upgrade the
/// session starts over on the TLS stream: no greeting (RFC 9051
/// §6.2.1), and nothing the client sent in clear is kept.
pub async fn run_upgradable<D, S>(dir: Arc<D>, io: S, tls: TlsState)
where
    D: Directory,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let mut framed = Framed::new(io, ImapCodec::new());
    if !greet(&*dir, &mut framed, true).await || !serve(&*dir, &mut framed, true).await {
        return;
    }
    // into_inner drops whatever the client pipelined after STARTTLS
    match tls.acceptor().accept(framed.into_inner()).await {
        Ok(stream) => {
            serve(&*dir, &mut Framed::new(stream, ImapCodec::new()), false).await;
        }
        Err(e) => tracing::debug!(error = %e, "imap: STARTTLS handshake failed"),
    }
}

/// Greeting per RFC 3501 §7.5. Sent before the client says anything.
async fn greet<D, S>(dir: &D, framed: &mut Io<S>, starttls: bool) -> bool
where
    D: Directory,
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let caps = auth::capabilities(starttls).join(" ");
    let greeting = format!("* OK [CAPABILITY {caps}] {} ready\r\n", dir.hostname());
    framed.send(greeting.into_bytes()).await.is_ok()
}

/// Runs commands until the connection ends (false) or the client is told
/// to begin TLS (true, only when `starttls`).
async fn serve<D, S>(dir: &D, framed: &mut Io<S>, starttls: bool) -> bool
where
    D: Directory,
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let mut session = State::<D::Account>::NotAuthed;
    // ENABLE QRESYNC is connection-scoped (RFC 7162 §3.2.5)
    let mut qresync = false;
    while let Some(frame) = framed.next().await {
        let Ok(input) = frame else { return false };
        let line = match input {
            ImapInput::Line(s) => s,
            // a literal outside an APPEND
            ImapInput::LiteralData(_) => continue,
        };
        let line = line.trim_end();
        let tag = line.split_once(' ').map_or("", |(t, _)| t).to_string();
        let command = match auth::early(line, &tag, starttls, framed).await {
            auth::Early::Parse => match parse_command(line) {
                Ok(cmd) => cmd.command,
                Err(e) => {
                    let parse_error = format_bad("*", &format!("parse: {e}"));
                    if framed.send(parse_error.into_bytes()).await.is_err() {
                        return false;
                    }
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
        let responses = dispatch(dir, &mut session, &tag, command, framed, &mut qresync).await;
        if !send_all(framed, responses).await || is_logout {
            return false;
        }
    }
    false
}

pub(super) async fn send_all<S>(framed: &mut Io<S>, out: Vec<String>) -> bool
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

/// Commands that change mail, which a read-only account refuses whatever
/// state the session is in.
fn changes_mail(cmd: &ImapCommand) -> bool {
    match cmd {
        ImapCommand::Store { .. }
        | ImapCommand::Copy { .. }
        | ImapCommand::Move { .. }
        | ImapCommand::Append { .. }
        | ImapCommand::Expunge
        | ImapCommand::Create { .. }
        | ImapCommand::Delete { .. }
        | ImapCommand::Rename { .. } => true,
        ImapCommand::Uid { subcommand } => matches!(
            **subcommand,
            ImapCommand::Store { .. } | ImapCommand::Copy { .. } | ImapCommand::Move { .. }
        ),
        _ => false,
    }
}

async fn dispatch<D, S>(
    dir: &D,
    session: &mut State<D::Account>,
    tag: &str,
    cmd: ImapCommand,
    framed: &mut Io<S>,
    qresync: &mut bool,
) -> Vec<String>
where
    D: Directory,
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    if changes_mail(&cmd)
        && let Some(reason) = session.account().and_then(|a| a.read_only())
    {
        return vec![format_no(tag, reason)];
    }
    match cmd {
        ImapCommand::Noop => vec![format_ok(tag, "NOOP completed")],
        ImapCommand::Logout => vec![
            format_bye("mailrs logging out"),
            format_ok(tag, "LOGOUT completed"),
        ],
        ImapCommand::Login { username, password } => {
            login(dir, session, tag, &username, &password).await
        }
        ImapCommand::List { pattern, .. } => list_response(session, tag, &pattern).await,
        ImapCommand::Select { mailbox } => select(session, tag, &mailbox, false, *qresync).await,
        ImapCommand::Examine { mailbox } => select(session, tag, &mailbox, true, *qresync).await,
        ImapCommand::Enable(caps) => enable(tag, &caps, qresync),
        ImapCommand::Close => close(session, tag),
        ImapCommand::Uid { subcommand } => uid(session, tag, *subcommand).await,
        ImapCommand::Expunge => expunge(session, tag, *qresync).await,
        ImapCommand::Append {
            mailbox,
            flags,
            literal_size,
        } => {
            append_flow(
                session,
                tag,
                framed,
                &mailbox,
                flags.as_deref(),
                literal_size,
            )
            .await
        }
        ImapCommand::Idle => idle_flow(session, tag, framed).await,
        ImapCommand::GetQuota { .. } => quota_response(session, tag).await,
        ImapCommand::GetQuotaRoot { mailbox } => {
            let mut out = vec![format!("* QUOTAROOT \"{mailbox}\" \"\"\r\n")];
            out.extend(quota_response(session, tag).await);
            out
        }
        cmd => sequenced(session, tag, cmd, false).await,
    }
}

/// UID FETCH / STORE / SEARCH / SORT / COPY / MOVE.
async fn uid<A: Account>(session: &mut State<A>, tag: &str, cmd: ImapCommand) -> Vec<String> {
    match cmd {
        ImapCommand::Fetch { .. }
        | ImapCommand::Store { .. }
        | ImapCommand::Search { .. }
        | ImapCommand::Sort { .. }
        | ImapCommand::Copy { .. }
        | ImapCommand::Move { .. } => sequenced(session, tag, cmd, true).await,
        _ => vec![format_bad(tag, "UID subcommand not supported")],
    }
}

/// The commands that address messages by sequence number or, under UID,
/// by uid.
async fn sequenced<A: Account>(
    session: &mut State<A>,
    tag: &str,
    cmd: ImapCommand,
    by_uid: bool,
) -> Vec<String> {
    match cmd {
        ImapCommand::Fetch {
            sequence,
            attributes,
        } => fetch_response(session, tag, &sequence, &attributes, by_uid).await,
        ImapCommand::Store {
            sequence,
            action,
            flags,
        } => store_response(session, tag, &sequence, &action, &flags, by_uid).await,
        ImapCommand::Search { criteria } => search_response(session, tag, &criteria, by_uid).await,
        ImapCommand::Sort {
            criteria,
            search_criteria,
            ..
        } => sort_response(session, tag, &criteria, &search_criteria, by_uid).await,
        ImapCommand::Copy { sequence, mailbox } => {
            copy_response(session, tag, &sequence, &mailbox, false, by_uid).await
        }
        ImapCommand::Move { sequence, mailbox } => {
            copy_response(session, tag, &sequence, &mailbox, true, by_uid).await
        }
        _ => vec![format_bad(tag, "command not implemented")],
    }
}

fn enable(tag: &str, caps: &[String], qresync: &mut bool) -> Vec<String> {
    let mut enabled = Vec::new();
    for c in caps {
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

async fn login<D: Directory>(
    dir: &D,
    session: &mut State<D::Account>,
    tag: &str,
    username: &str,
    password: &str,
) -> Vec<String> {
    if !matches!(session, State::NotAuthed) {
        return vec![format_bad(tag, "already authenticated")];
    }
    match dir.login(username, password, Protocol::Imap).await {
        Ok(Some(account)) => {
            *session = State::Authed {
                account: Arc::new(account),
            };
            vec![format_ok(tag, "LOGIN completed")]
        }
        Ok(None) => vec![format_no(tag, "invalid credentials")],
        Err(e) => vec![no(tag, "LOGIN", &e)],
    }
}

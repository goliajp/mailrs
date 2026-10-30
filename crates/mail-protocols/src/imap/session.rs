//! Per-connection IMAP session — state machine + command dispatch.
//!
//! Structure follows RFC 3501 §3: NotAuthenticated → Authenticated →
//! Selected → Logout. LOGIN flips NotAuthenticated → Authenticated;
//! SELECT / EXAMINE flip Authenticated → Selected; LOGOUT ends the
//! connection. Unknown / unsupported commands answer `BAD`,
//! unauthenticated commands answer `NO`.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use mailrs_imap_codec::{ImapCodec, ImapInput, TooLong};
use mailrs_imap_proto::{ImapCommand, format_bad, format_no, format_ok, parse_command};
use mailrs_smtp_proto::MAX_MESSAGE_SIZE;
use mailrs_tls_reload::TlsState;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use super::auth;
use super::commands::dispatch;
use crate::backend::{Account, Directory, Error, Message};
use mailrs_auth_guard::signins::{ENDED, SignedIn, ended};

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
    let mut framed = Framed::new(io, codec());
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
    let mut framed = Framed::new(io, codec());
    if !greet(&*dir, &mut framed, true).await || !serve(&*dir, &mut framed, true).await {
        return;
    }
    // into_inner drops whatever the client pipelined after STARTTLS
    match tls.acceptor().accept(framed.into_inner()).await {
        Ok(stream) => {
            serve(&*dir, &mut Framed::new(stream, codec()), false).await;
        }
        Err(e) => tracing::debug!(error = %e, "imap: STARTTLS handshake failed"),
    }
}

/// Lines up to the codec's 64 KiB, before sign-in as after it (RFC 7162
/// §4 asks for at least 8 KiB), and literals up to the largest message
/// the receiver takes.
fn codec() -> ImapCodec {
    ImapCodec::new().with_max_literal(MAX_MESSAGE_SIZE as u32)
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
    let mut signed_in: Option<SignedIn> = None;
    loop {
        let frame = tokio::select! {
            frame = framed.next() => frame,
            () = ended(signed_in.as_ref()) => {
                let bye = format!("* BYE [ALERT] {ENDED}\r\n");
                drop(framed.send(bye.into_bytes()).await);
                return false;
            }
        };
        let Some(frame) = frame else { break };
        let input = match frame {
            Ok(input) => input,
            Err(e) => {
                if let Some(what) = e.get_ref().and_then(|e| e.downcast_ref::<TooLong>()) {
                    // the connection closes whether or not this is read
                    let bad = format!("* BAD [TOOBIG] {what}, closing\r\n");
                    drop(framed.send(bad.into_bytes()).await);
                }
                return false;
            }
        };
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
        let parsed = (command, line);
        let responses = dispatch(
            dir,
            &mut session,
            &tag,
            parsed,
            framed,
            &mut qresync,
            signed_in.as_ref(),
        )
        .await;
        if signed_in.is_none()
            && let (Some(signins), Some(account)) = (dir.signins(), session.account())
        {
            signed_in = Some(signins.enter(account.user()));
        }
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

//! `550 5.1.1` at RCPT for a local address nobody receives.
//!
//! Only addresses on a domain the receiver lists as local are asked about,
//! signed-in submission included; remote recipients, mock addresses and a
//! mock namespace's sandbox session keep their own handling.

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_core_sidestate::families::mock;
use mailrs_smtp_codec::SmtpCodec;
use mailrs_smtp_proto::Command;
use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::{Session, State};

use super::super::{ConnectionContext, SessionAction};
use super::reply::{code, handle_reply};

/// `Some` when this RCPT was refused and the reply already written.
pub(in crate::smtp_session) async fn refuse_unknown_rcpt<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    cmd: &Command<'_>,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> Option<SessionAction>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let check = ctx.recipient_check.as_ref()?;
    let rcpt = session.pending_recipient(cmd)?;
    if !is_asked_about(&rcpt, &session.state, ctx) || !check.is_unknown(&rcpt).await {
        return None;
    }
    let resp = Response::new(550, Some(code(5, 1, 1)), format!("<{rcpt}>: no such user"));
    Some(handle_reply(framed, session, resp, ctx, conn_id).await)
}

fn is_asked_about(rcpt: &str, state: &State, ctx: &ConnectionContext) -> bool {
    let Some((_, domain)) = rcpt.rsplit_once('@') else {
        return false;
    };
    if mock::is_mock(rcpt) || in_sandbox(state) {
        return false;
    }
    // an empty list means every domain is local; nothing is asked then,
    // or a signed-in user's mail to anywhere would be checked as ours
    ctx.runtime
        .local_domains
        .current()
        .iter()
        .any(|d| d.eq_ignore_ascii_case(domain))
}

fn in_sandbox(state: &State) -> bool {
    let username = match state {
        State::MailFrom { username, .. } | State::RcptTo { username, .. } => username.as_deref(),
        _ => None,
    };
    username.is_some_and(|u| crate::smtp_session::credentials::sandbox_namespace(u).is_some())
}

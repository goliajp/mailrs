//! `550 5.7.1` at MAIL FROM for a signed-in account whose outbound mail is
//! paused, saying whom to ask. The sender holds such mail anyway; this
//! tells the mail client at once instead of accepting what will not leave.

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_core_sidestate::families::outbound::pause;
use mailrs_smtp_codec::SmtpCodec;
use mailrs_smtp_proto::Command;
use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::{Session, State};

use super::super::{ConnectionContext, SessionAction};
use super::reply::{code, handle_reply};

/// `Some` when this MAIL FROM was refused and the reply already written.
pub(in crate::smtp_session) async fn refuse_paused_sender<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    cmd: &Command<'_>,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> Option<SessionAction>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if !matches!(cmd, Command::MailFrom { .. }) {
        return None;
    }
    let State::Authenticated { username, .. } = &session.state else {
        return None;
    };
    let client = ctx.spam_lists_client.clone()?;
    let account = username.to_ascii_lowercase();
    let paused =
        tokio::task::spawn_blocking(move || client.with_conn(|c| pause::is_paused(c, &account)))
            .await;
    if !matches!(paused, Ok(Ok(true))) {
        return None;
    }
    let resp = Response::new(
        550,
        Some(code(5, 7, 1)),
        mailrs_auth_guard::password::outbound_paused_notice(),
    );
    Some(handle_reply(framed, session, resp, ctx, conn_id).await)
}

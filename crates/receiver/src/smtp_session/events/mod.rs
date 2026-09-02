use std::net::SocketAddr;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_smtp_proto::session::{Event, Session};

use mailrs_smtp_codec::SmtpCodec;

use super::auth;
use super::{ConnectionContext, SessionAction};

mod data;
mod reply;
mod shutdown;
mod starttls;

pub(super) async fn handle_event<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    event: Event,
    addr: SocketAddr,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match event {
        Event::Reply(resp) => reply::handle_reply(framed, session, resp, ctx, conn_id).await,
        Event::NeedData {
            reverse_path,
            forward_paths,
        } => {
            data::handle_need_data(
                framed,
                session,
                reverse_path,
                forward_paths,
                addr,
                ctx,
                conn_id,
            )
            .await
        }
        // Emitted here rather than inside the two helpers, which take
        // neither `ctx` nor `conn_id`. Every branch that puts a line on
        // the wire must announce it, or the live monitor shows a
        // transcript with holes in it — QUIT's `221` and STARTTLS's
        // `220` were both missing, and a conversation ending on the
        // client's QUIT with no answer reads as a dropped connection.
        Event::Shutdown(resp) => {
            emit_response_sent(ctx, conn_id, &resp, session);
            shutdown::handle_shutdown(framed, resp).await
        }
        Event::StartTls(resp) => {
            emit_response_sent(ctx, conn_id, &resp, session);
            starttls::handle_starttls(framed, resp).await
        }
        Event::NeedAuth { username, password } => {
            auth::handle_need_auth(framed, session, username, password, addr, ctx, conn_id).await
        }
        Event::AuthChallenge { response, step } => {
            auth::handle_auth_challenge(framed, session, response, step, addr, ctx, conn_id).await
        }
    }
}

/// Announce a response on the session trace.
///
/// Emitted *before* the write, matching `handle_reply` — the monitor is
/// a transcript of what the server decided to say, and a write that
/// then fails closes the connection, which the trace shows next.
pub(super) fn emit_response_sent(
    ctx: &ConnectionContext,
    conn_id: u64,
    resp: &mailrs_smtp_proto::response::Response,
    session: &Session,
) {
    ctx.event_bus
        .emit(mailrs_core::event_bus::SmtpEvent::ResponseSent {
            id: conn_id,
            response: resp.format().trim_end().to_string(),
            state_after: format!("{:?}", session.state),
        });
}

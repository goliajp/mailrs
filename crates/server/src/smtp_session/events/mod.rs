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
        Event::Shutdown(resp) => shutdown::handle_shutdown(framed, resp).await,
        Event::StartTls(resp) => starttls::handle_starttls(framed, resp).await,
        Event::NeedAuth { username, password } => {
            auth::handle_need_auth(framed, session, username, password, addr, ctx, conn_id).await
        }
        Event::AuthChallenge { response, step } => {
            auth::handle_auth_challenge(framed, session, response, step, addr, ctx, conn_id).await
        }
    }
}

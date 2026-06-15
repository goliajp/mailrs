use futures_util::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::Session;

use mailrs_core::event_bus::SmtpEvent;
use mailrs_smtp_codec::SmtpCodec;

use super::super::{ConnectionContext, SessionAction};

pub(super) async fn handle_reply<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    resp: Response,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    ctx.event_bus.emit(SmtpEvent::ResponseSent {
        id: conn_id,
        response: resp.format().trim_end().to_string(),
        state_after: format!("{:?}", session.state),
    });
    if framed.send(resp.format()).await.is_err() {
        return SessionAction::Close;
    }
    SessionAction::Continue
}

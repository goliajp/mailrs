use futures_util::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::Response;

use mailrs_smtp_codec::SmtpCodec;

use super::super::SessionAction;

pub(super) async fn handle_shutdown<S>(
    framed: &mut Framed<S, SmtpCodec>,
    resp: Response,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let _ = framed.send(resp.format()).await;
    SessionAction::Close
}

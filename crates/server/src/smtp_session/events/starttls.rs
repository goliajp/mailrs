use futures_util::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::Response;

use mailrs_smtp_codec::SmtpCodec;

use super::super::SessionAction;

pub(super) async fn handle_starttls<S>(
    framed: &mut Framed<S, SmtpCodec>,
    resp: Response,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if framed.send(resp.format()).await.is_err() {
        return SessionAction::Close;
    }
    SessionAction::UpgradeTls
}

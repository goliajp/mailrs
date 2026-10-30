use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::{Session, State};
use mailrs_smtp_proto::unstuff_data;

use mailrs_smtp_codec::{SmtpCodec, SmtpInput};

use super::super::headers::format_received_header;
use super::super::{ConnectionContext, DATA_TIMEOUT, SessionAction};
use super::reply::{code, send_reply};

mod antispam;
mod mock_gate;
mod sender_gate;
mod spool_accept;

use antispam::{AntiSpamOutcome, run_antispam};

pub(super) async fn handle_need_data<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    reverse_path: String,
    forward_paths: Vec<String>,
    addr: SocketAddr,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let resp = Response::data_start();
    if framed.send(resp.format()).await.is_err() {
        return SessionAction::Close;
    }
    framed.codec_mut().enter_data_mode();

    // check if sender is authenticated (needed for outbound)
    let is_authenticated = matches!(
        session.state,
        State::Authenticated { .. }
            | State::MailFrom {
                username: Some(_),
                ..
            }
            | State::RcptTo {
                username: Some(_),
                ..
            }
    );

    match tokio::time::timeout(DATA_TIMEOUT, framed.next()).await {
        Ok(Some(Ok(SmtpInput::Data(raw)))) => {
            let body = unstuff_data(&raw);
            if let sender_gate::SenderGate::Refuse(resp) =
                sender_gate::check(&session.state, &reverse_path, &body, addr.ip(), ctx).await
            {
                return send_reply(framed, resp).await;
            }
            let received = format_received_header(
                session.client_domain(),
                &ctx.hostname,
                forward_paths.first().map(|s| s.as_str()).unwrap_or(""),
                &addr,
            );
            let mut full_message = received.into_bytes();
            full_message.extend_from_slice(&body);

            let mut target_folder = "INBOX";
            let mut fraud_verdict: Option<String> = None;
            let sandbox_ns = mock_gate::sandbox_namespace(&session.state);
            let all_mock = match mock_gate::check(&forward_paths, sandbox_ns.is_some(), ctx).await {
                mock_gate::MockGate::Mixed => false,
                mock_gate::MockGate::AllMock => true,
                mock_gate::MockGate::Refuse(resp) => return send_reply(framed, resp).await,
            };
            if !is_authenticated && !all_mock && ctx.antispam_enabled {
                match run_antispam(
                    &session.state,
                    addr,
                    &reverse_path,
                    &forward_paths,
                    full_message,
                    conn_id,
                    ctx,
                )
                .await
                {
                    AntiSpamOutcome::Reject(resp) => return send_reply(framed, resp).await,
                    AntiSpamOutcome::Continue {
                        full_message: new_msg,
                        target_folder: tf,
                        fraud_verdict: v,
                    } => {
                        full_message = new_msg;
                        target_folder = tf;
                        fraud_verdict = v;
                    }
                }
            }

            spool_accept::handle_spool_mode(
                framed,
                &reverse_path,
                &forward_paths,
                is_authenticated,
                conn_id,
                target_folder,
                fraud_verdict,
                sandbox_ns,
                &full_message,
                ctx,
            )
            .await
        }
        Ok(Some(Ok(SmtpInput::DataRejected))) => {
            let resp = Response::new(
                550,
                Some(code(5, 7, 7)),
                "SMTP smuggling detected, message rejected",
            );
            tracing::warn!(
                event = "smtp_smuggling",
                id = conn_id,
                from = %reverse_path,
                "SMTP smuggling attempt detected"
            );
            send_reply(framed, resp).await
        }
        // the codec's only InvalidData: the payload passed the size limit
        Ok(Some(Err(e))) if e.kind() == std::io::ErrorKind::InvalidData => {
            let _ = framed.send(Response::too_large().format()).await;
            SessionAction::Close
        }
        Err(_) => {
            // data transfer timeout
            let _ = framed
                .send(Response::new(421, None, "Data timeout, closing connection").format())
                .await;
            SessionAction::Close
        }
        _ => SessionAction::Close,
    }
}

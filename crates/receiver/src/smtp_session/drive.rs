//! One command off the wire and the session's answer to it.

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_core::event_bus::SmtpEvent;
use mailrs_smtp_codec::{SmtpCodec, SmtpInput};
use mailrs_smtp_proto::parse_command;
use mailrs_smtp_proto::response::{Response, format_ehlo_response};
use mailrs_smtp_proto::session::{Event, Session};

use super::events::{handle_event, refuse_unknown_rcpt};
use super::{CONNECTION_TIMEOUT, ConnectionContext, SessionAction};

/// process one command from the stream, return what to do next
pub(super) async fn drive_session<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    addr: SocketAddr,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let result = match tokio::time::timeout(CONNECTION_TIMEOUT, framed.next()).await {
        Ok(Some(result)) => result,
        Ok(None) => return SessionAction::Close,
        Err(_) => {
            let _ = framed
                .send(Response::new(421, None, "Idle timeout, closing connection").format())
                .await;
            return SessionAction::Close;
        }
    };
    let input = match result {
        Ok(input) => input,
        Err(_) => return SessionAction::Close,
    };

    match input {
        SmtpInput::Command(line) => {
            ctx.event_bus.emit(SmtpEvent::CommandReceived {
                id: conn_id,
                command: line.clone(),
                state_before: format!("{:?}", session.state),
            });

            match parse_command(&line) {
                Ok(cmd) => {
                    let is_ehlo = matches!(
                        cmd,
                        mailrs_smtp_proto::Command::Ehlo(_) | mailrs_smtp_proto::Command::Helo(_)
                    );
                    if let Some(action) =
                        refuse_unknown_rcpt(framed, session, &cmd, ctx, conn_id).await
                    {
                        return action;
                    }
                    let event = session.handle_command(&cmd);

                    // EHLO/HELO with 250 → multiline response
                    if is_ehlo && matches!(event, Event::Reply(ref r) if r.code == 250) {
                        let caps = session.capabilities();
                        let resp = format_ehlo_response(&session.hostname, &caps);
                        ctx.event_bus.emit(SmtpEvent::ResponseSent {
                            id: conn_id,
                            response: resp.clone(),
                            state_after: format!("{:?}", session.state),
                        });
                        if framed.send(resp).await.is_err() {
                            return SessionAction::Close;
                        }
                        return SessionAction::Continue;
                    }

                    handle_event(framed, session, event, addr, ctx, conn_id).await
                }
                Err(_) => {
                    let resp = Response::syntax_error();
                    if framed.send(resp.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    SessionAction::Continue
                }
            }
        }
        SmtpInput::Data(_) | SmtpInput::DataRejected => SessionAction::Close,
    }
}

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::{AuthStep, Event, Session};

use mailrs_auth_guard::login::Verdict;
use mailrs_core::event_bus::SmtpEvent;
use mailrs_smtp_codec::{SmtpCodec, SmtpInput};

use super::credentials::verify_credentials;
use super::{CONNECTION_TIMEOUT, ConnectionContext, SessionAction};

pub(super) async fn handle_need_auth<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    username: String,
    password: String,
    addr: SocketAddr,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    sign_in(framed, session, username, password, addr, ctx, conn_id).await
}

pub(super) async fn handle_auth_challenge<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    response: Response,
    step: AuthStep,
    addr: SocketAddr,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if framed.send(response.format()).await.is_err() {
        return SessionAction::Close;
    }
    handle_auth_continuation(framed, session, step, addr, ctx, conn_id).await
}

pub(super) async fn handle_auth_continuation<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    step: AuthStep,
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
            let event = session.handle_auth_response(&line, &step);
            match event {
                Event::NeedAuth { username, password } => {
                    sign_in(framed, session, username, password, addr, ctx, conn_id).await
                }
                Event::AuthChallenge {
                    response,
                    step: next_step,
                } => {
                    if framed.send(response.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    Box::pin(handle_auth_continuation(
                        framed, session, next_step, addr, ctx, conn_id,
                    ))
                    .await
                }
                Event::Reply(resp) => {
                    if framed.send(resp.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    SessionAction::Continue
                }
                _ => SessionAction::Close,
            }
        }
        _ => SessionAction::Close,
    }
}

/// Verify the credentials unless the login guard has locked this source
/// or account; a locked attempt never reaches the password check.
async fn sign_in<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    username: String,
    password: String,
    addr: SocketAddr,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let source = addr.ip().to_canonical();
    let verdict = ctx.auth_guard.check(source, &username).await;
    let resp = if let Verdict::Locked { .. } = verdict {
        tracing::warn!(event = "auth", channel = "smtp", %source, account = %username, result = "locked");
        Response::new(
            421,
            Some(mailrs_smtp_proto::EnhancedCode {
                class: 4,
                subject: 7,
                detail: 0,
            }),
            format!(
                "Too many failed sign-in attempts, try again in {} minutes",
                verdict.retry_after_minutes()
            ),
        )
    } else if verify_credentials(ctx, &username, &password).await {
        ctx.auth_guard.succeeded(source, &username).await;
        session.set_authenticated(username.clone());
        ctx.event_bus.emit(SmtpEvent::Authenticated {
            id: conn_id,
            username,
        });
        Response::auth_ok()
    } else {
        tracing::warn!(event = "auth", channel = "smtp", %source, account = %username, result = "bad-password");
        ctx.auth_guard.failed(source, &username).await;
        Response::auth_failed()
    };
    if framed.send(resp.format()).await.is_err() {
        return SessionAction::Close;
    }
    SessionAction::Continue
}

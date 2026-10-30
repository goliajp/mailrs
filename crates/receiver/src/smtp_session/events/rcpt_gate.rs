//! Refusals at RCPT for local addresses: `550 5.1.1` for one nobody
//! receives, and `550 5.7.1` for an SRS address reached by anything but a
//! bounce.
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
    let rcpt = session.pending_recipient(cmd)?;
    if is_local(&rcpt, ctx) && srs_without_bounce(&rcpt, &session.state) {
        let resp = Response::new(
            550,
            Some(code(5, 7, 1)),
            format!("<{rcpt}>: this address takes only bounces"),
        );
        return Some(handle_reply(framed, session, resp, ctx, conn_id).await);
    }
    let check = ctx.recipient_check.as_ref()?;
    if !is_asked_about(&rcpt, &session.state, ctx) || !check.is_unknown(&rcpt).await {
        return None;
    }
    let resp = Response::new(550, Some(code(5, 1, 1)), format!("<{rcpt}>: no such user"));
    Some(handle_reply(framed, session, resp, ctx, conn_id).await)
}

/// An SRS address is where a bounce of mail we forwarded comes back, and
/// only a bounce (an empty reverse path) is relayed on from it. Anything
/// else would use it to send through us to the address it hides.
fn srs_without_bounce(rcpt: &str, state: &State) -> bool {
    let reverse_path = match state {
        State::MailFrom { reverse_path, .. } | State::RcptTo { reverse_path, .. } => reverse_path,
        _ => return false,
    };
    let is_srs = rcpt
        .get(..5)
        .is_some_and(|p| p.eq_ignore_ascii_case("SRS0="));
    is_srs && !reverse_path.is_empty()
}

fn is_asked_about(rcpt: &str, state: &State, ctx: &ConnectionContext) -> bool {
    if mock::is_mock(rcpt) || in_sandbox(state) {
        return false;
    }
    is_local(rcpt, ctx)
}

fn is_local(rcpt: &str, ctx: &ConnectionContext) -> bool {
    let Some((_, domain)) = rcpt.rsplit_once('@') else {
        return false;
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rcpt_state(reverse_path: &str) -> State {
        State::RcptTo {
            domain: "mx.example".into(),
            username: None,
            reverse_path: reverse_path.into(),
            params: Vec::new(),
            forward_paths: Vec::new(),
        }
    }

    #[test]
    fn an_srs_address_takes_only_bounces() {
        let srs = "SRS0=HHH=TT=origin.example=alice@golia.jp";
        assert!(srs_without_bounce(srs, &rcpt_state("someone@spam.example")));
        assert!(srs_without_bounce(
            &srs.to_lowercase(),
            &rcpt_state("x@y.example")
        ));
        assert!(!srs_without_bounce(srs, &rcpt_state("")));
        assert!(!srs_without_bounce(
            "alice@golia.jp",
            &rcpt_state("x@y.example")
        ));
    }
}

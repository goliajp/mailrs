//! Signed-in submission may only use From addresses its account may send
//! as, the same rule the web send applies
//! (`EffectivePermissionsResponse::may_send_as`). Both the envelope sender
//! and the `From:` header are checked, once the message is in, and every
//! decision is logged with the account, both addresses and the client.
//!
//! A namespace's sandbox login keeps its mail in the sandbox, and may use
//! only its own namespace's addresses. A login known only to `users.toml`
//! has no permissions to ask for and may use only its own address.

use std::net::IpAddr;

use mailrs_core_api::method::admin::EffectivePermissionsResponse;
use mailrs_core_sidestate::families::mock;
use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::State;

use super::super::super::ConnectionContext;
use super::super::reply::code;
use crate::config::SubmissionFromCheck;

pub(super) enum SenderGate {
    Pass,
    Refuse(Response),
}

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Allowed,
    Refused(&'static str),
    /// The account's permissions could not be read.
    Unknown(String),
}

pub(super) async fn check(
    state: &State,
    reverse_path: &str,
    message: &[u8],
    client: IpAddr,
    ctx: &ConnectionContext,
) -> SenderGate {
    let Some(user) = username(state) else {
        return SenderGate::Pass;
    };
    let envelope = reverse_path.trim().trim_matches(['<', '>']).trim();
    let header = mailrs_inbound::identity::from_address(message);
    let verdict = decide(user, envelope, &header, ctx).await;
    let enforce = ctx.submission_from_check == SubmissionFromCheck::Enforce;
    match &verdict {
        Verdict::Allowed => tracing::info!(
            event = "submission_sender",
            user,
            envelope,
            header_from = %header,
            %client,
            verdict = "allowed",
        ),
        Verdict::Refused(reason) => tracing::warn!(
            event = "submission_sender",
            user,
            envelope,
            header_from = %header,
            %client,
            verdict = if enforce { "refused" } else { "would_refuse" },
            reason,
        ),
        Verdict::Unknown(error) => tracing::warn!(
            event = "submission_sender",
            user,
            envelope,
            header_from = %header,
            %client,
            verdict = if enforce { "deferred" } else { "would_defer" },
            reason = "permissions unreadable",
            error = %error,
        ),
    }
    match (verdict, enforce) {
        (Verdict::Allowed, _) | (_, false) => SenderGate::Pass,
        (Verdict::Refused(reason), true) => SenderGate::Refuse(Response::new(
            550,
            Some(code(5, 7, 1)),
            format!("{user} may not send as this address: {reason}"),
        )),
        (Verdict::Unknown(_), true) => SenderGate::Refuse(Response::new(
            451,
            Some(code(4, 3, 0)),
            "cannot check the sender right now, try again later",
        )),
    }
}

fn username(state: &State) -> Option<&str> {
    match state {
        State::MailFrom { username, .. } | State::RcptTo { username, .. } => username.as_deref(),
        _ => None,
    }
}

async fn decide(user: &str, envelope: &str, header: &str, ctx: &ConnectionContext) -> Verdict {
    if crate::smtp_session::credentials::sandbox_namespace(user).is_some() {
        return judge(user, envelope, header, Ok(None));
    }
    let perms = match &ctx.credential_store {
        Some(store) => store
            .effective_permissions(user)
            .await
            .map_err(|e| e.to_string()),
        None => Ok(None),
    };
    judge(user, envelope, header, perms)
}

/// `perms` is what the core says `user` may do: `Ok(None)` for a login it
/// does not know (a sandbox, or a `users.toml` entry).
fn judge(
    user: &str,
    envelope: &str,
    header: &str,
    perms: Result<Option<EffectivePermissionsResponse>, String>,
) -> Verdict {
    if let Some(ns) = crate::smtp_session::credentials::sandbox_namespace(user) {
        let ours = |a: &str| a.is_empty() || mock::parse(a).is_some_and(|m| m.namespace == ns);
        return verdict_for(ours(envelope), ours(header));
    }
    let perms = match perms {
        Ok(p) => p,
        Err(e) => return Verdict::Unknown(e),
    };
    let may = |a: &str| match &perms {
        _ if a.is_empty() => true,
        Some(p) => p.may_send_as(a),
        None => a.eq_ignore_ascii_case(user),
    };
    verdict_for(may(envelope), may(header))
}

/// An empty envelope sender (a bounce) and a message without `From:` are
/// not addresses to check; the caller passes them as allowed.
fn verdict_for(envelope_ok: bool, header_ok: bool) -> Verdict {
    match (envelope_ok, header_ok) {
        (true, true) => Verdict::Allowed,
        (false, _) => Verdict::Refused("envelope sender"),
        (true, false) => Verdict::Refused("From header"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perms(
        is_super: bool,
        send_as: &[&str],
    ) -> Result<Option<EffectivePermissionsResponse>, String> {
        Ok(Some(EffectivePermissionsResponse {
            address: "xiaxi@golia.jp".into(),
            permissions: Vec::new(),
            groups: Vec::new(),
            is_super,
            send_as: send_as.iter().map(|s| s.to_string()).collect(),
        }))
    }

    const ME: &str = "xiaxi@golia.jp";

    #[test]
    fn an_account_sends_as_itself_and_its_grants() {
        assert_eq!(judge(ME, ME, ME, perms(false, &[])), Verdict::Allowed);
        assert_eq!(
            judge(
                ME,
                "Xiaxi@Golia.jp",
                "team@golia.jp",
                perms(false, &["team@golia.jp"])
            ),
            Verdict::Allowed
        );
    }

    #[test]
    fn a_guessed_password_cannot_send_as_someone_else() {
        assert_eq!(
            judge(ME, "lihao@golia.jp", ME, perms(false, &[])),
            Verdict::Refused("envelope sender")
        );
        assert_eq!(
            judge(ME, ME, "noreply@golia.jp", perms(false, &[])),
            Verdict::Refused("From header")
        );
    }

    #[test]
    fn a_super_admin_sends_as_anyone() {
        assert_eq!(
            judge(ME, "ceo@golia.jp", "anyone@else.test", perms(true, &[])),
            Verdict::Allowed
        );
    }

    #[test]
    fn a_bounce_and_a_message_without_from_check_only_what_is_there() {
        assert_eq!(judge(ME, "", ME, perms(false, &[])), Verdict::Allowed);
        assert_eq!(judge(ME, ME, "", perms(false, &[])), Verdict::Allowed);
        assert_eq!(
            judge(ME, "", "lihao@golia.jp", perms(false, &[])),
            Verdict::Refused("From header")
        );
    }

    #[test]
    fn a_login_the_core_does_not_know_sends_only_as_itself() {
        assert_eq!(
            judge("sentori@golia.jp", "sentori@golia.jp", "", Ok(None)),
            Verdict::Allowed
        );
        assert_eq!(
            judge("sentori@golia.jp", "noreply@golia.jp", "", Ok(None)),
            Verdict::Refused("envelope sender")
        );
    }

    #[test]
    fn unreadable_permissions_defer_rather_than_guess() {
        assert_eq!(
            judge(ME, ME, ME, Err("core down".into())),
            Verdict::Unknown("core down".into())
        );
    }
}

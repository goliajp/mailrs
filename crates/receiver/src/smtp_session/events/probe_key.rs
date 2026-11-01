//! Who an unknown recipient at RCPT is counted against, and what is
//! logged when that sender is cut off. The limits are `probe_guard`'s.

use std::net::IpAddr;
use std::time::Instant;

use mailrs_smtp_proto::session::{Session, State};

use super::super::ConnectionContext;
use super::super::probe_guard::Key;

/// The signed-in account; else IP and MAIL FROM domain when that domain's
/// SPF passes for the IP; else the IP.
pub(super) async fn of(
    session: &Session,
    ctx: &ConnectionContext,
    ip: IpAddr,
    now: Instant,
) -> Key {
    let (username, reverse_path) = match &session.state {
        State::MailFrom {
            username,
            reverse_path,
            ..
        }
        | State::RcptTo {
            username,
            reverse_path,
            ..
        } => (username.as_deref(), reverse_path.as_str()),
        _ => (None, ""),
    };
    if let Some(user) = username {
        return Key::Account(user.to_ascii_lowercase());
    }
    let Some((_, domain)) = reverse_path.rsplit_once('@') else {
        return Key::Ip(ip);
    };
    let domain = domain.to_ascii_lowercase();
    match spf_passes(session, ctx, ip, reverse_path, &domain, now).await {
        true => Key::IpDomain(ip, domain),
        false => Key::Ip(ip),
    }
}

async fn spf_passes(
    session: &Session,
    ctx: &ConnectionContext,
    ip: IpAddr,
    mail_from: &str,
    domain: &str,
    now: Instant,
) -> bool {
    let Some(resolver) = ctx.rcpt_spf.as_ref() else {
        return false;
    };
    if let Some(pass) = ctx.unknown_rcpt.spf_seen(ip, domain, now) {
        return pass;
    }
    let input = mailrs_spf::VerifyInput {
        ip,
        helo: session.client_domain().to_string(),
        mail_from: mail_from.to_string(),
    };
    let pass = mailrs_spf::verify(resolver.as_ref(), &input).await == mailrs_spf::SpfResult::Pass;
    ctx.unknown_rcpt.remember_spf(ip, domain, pass, now);
    pass
}

/// Log and count one refusal of a cut-off sender. A signed-in account
/// cut off is also its own event, which is alerted on: an account that
/// guesses addresses is likely in the wrong hands.
pub(super) fn tripped(ctx: &ConnectionContext, key: &Key, ip: IpAddr, now: Instant) {
    let count = ctx.unknown_rcpt.count(key, now);
    ctx.metrics.probe_guard_tripped();
    tracing::warn!(
        event = "probe_guard_tripped",
        key = key.kind(),
        %ip,
        domain = key.domain(),
        count,
        "unknown local recipients past the limit; refusing"
    );
    if let Key::Account(account) = key {
        tracing::warn!(
            event = "probe_guard_account",
            account = %mailrs_auth_guard::login::loggable_account(account),
            %ip,
            count,
            "a signed-in account guessed too many unknown local recipients"
        );
    }
}

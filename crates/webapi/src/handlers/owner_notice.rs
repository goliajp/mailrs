//! Telling an account's owner what just happened to their account, at the
//! account and at its recovery address: if the account is in someone
//! else's hands they can delete the first copy, not the second.

use crate::WebState;
use crate::security_mail::{Notice, Reader, owner_mail};

pub(crate) use crate::security_mail::owner::Origin;

/// Where it came from, from the request.
pub(crate) fn origin_of(headers: &axum::http::HeaderMap, ip: &str) -> Origin {
    Origin {
        ip: ip.to_string(),
        user_agent: headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.chars().take(300).collect())
            .unwrap_or_default(),
    }
}

/// Queue the mail about `notice` to `address`, its recovery address (when
/// `state` is there to look it up) and `also`. A failure is logged: the
/// change it reports has already been made.
pub(crate) async fn notify_owner(
    state: Option<&WebState>,
    address: &str,
    notice: Notice,
    origin: Origin,
    also: &[String],
) {
    let host = std::env::var("MAILRS_HOSTNAME").unwrap_or_default();
    if host.is_empty() {
        tracing::error!(%address, "MAILRS_HOSTNAME unset — the owner is not told");
        return;
    }
    let from = std::env::var(crate::login_guard::ALERT_FROM)
        .ok()
        .filter(|f| f.contains('@'))
        .unwrap_or_else(|| format!("noreply@{host}"));
    let mut to = vec![address.to_string()];
    let recovery = match state {
        Some(state) => recovery_address(state, address).await,
        None => None,
    };
    for extra in recovery.into_iter().chain(also.iter().cloned()) {
        if !to.iter().any(|t| t.eq_ignore_ascii_case(&extra)) {
            to.push(extra);
        }
    }
    let now = crate::handlers::complete::now_secs().max(0) as u64;
    let mail = owner_mail(&notice, &origin, &host, &Reader::from_env(), now);
    let parts = crate::handlers::compose::ComposeParts {
        from: from.clone(),
        to: to.clone(),
        subject: mail.subject,
        body: mail.text,
        html_body: mail.html,
        ..Default::default()
    };
    if crate::handlers::compose::breaks_a_header_line(&parts) {
        tracing::warn!(%address, ?notice, "the owner could not be told: an address carries a line break");
        return;
    }
    let (_, message) = crate::handlers::compose::build_rfc5322(&parts, &from);
    if let Err(code) = crate::handlers::send_queue::enqueue_security_alert(&from, &to, &message) {
        tracing::warn!(%address, %code, ?notice, "the owner could not be told");
    }
}

pub(crate) async fn recovery_address(state: &WebState, address: &str) -> Option<String> {
    let acct = state.core.get_account_with_hash(address).await.ok()?;
    let v = serde_json::to_value(&acct).ok()?;
    v.get("recovery_email")?
        .as_str()
        .map(str::trim)
        .filter(|s| s.contains('@'))
        .map(str::to_string)
}

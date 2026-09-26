//! What this sweep was configured with, and what it says when it was
//! not.
//!
//! Every value here is read from the process's own environment or its
//! own store, and every one of them has been missing in production at
//! least once: `MAILRS_ORG_NAMES` was set on the receiver and not on
//! this process, and half the impersonation check was silently off for
//! a day. So a reader that comes up empty says so in the log rather
//! than letting the sweep report "found nothing".

use super::super::prelude::*;

/// The display name on every account row.
///
/// Read here rather than configured, because the store is the
/// authority on who has an account and a variable is a second copy
/// that can drift from it.
pub(super) fn account_display_names(state: &Arc<FastcoreState>) -> Vec<String> {
    let Ok(addrs) = state.mailbox.list_account_addresses() else {
        return Vec::new();
    };
    addrs
        .iter()
        .filter_map(|a| state.mailbox.get_account_blob(a).ok().flatten())
        .filter_map(|blob| serde_json::from_str::<serde_json::Value>(&blob).ok())
        .filter_map(|v| {
            v.get("display_name")
                .and_then(|d| d.as_str())
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string)
        })
        .collect()
}

/// The fraud policy this process was configured with, and a warning
/// when half of it is missing.
///
/// A sweep with no org names cannot fire the impersonation rule, and
/// its `found` count comes back looking like an answer. Say so.
pub(super) fn policy_from_env(state: &Arc<FastcoreState>) -> mailrs_fraud::Policy {
    let policy = mailrs_fraud::Policy {
        org_names: csv_env("MAILRS_ORG_NAMES"),
        our_domains: csv_env("MAILRS_LOCAL_DOMAINS"),
        allowed_domains: csv_env("MAILRS_ORG_NAME_ALLOWED_DOMAINS"),
        // **From the account rows, not from the environment.** A
        // deployment knows who holds an account on it, and asking an
        // operator to keep a second copy in a variable is asking for
        // the thing that already happened once: `MAILRS_ORG_NAMES`
        // was set on the receiver and not on this process, so half
        // the impersonation check was silently off for a day.
        account_names: account_display_names(state),
    };
    if policy.account_names.is_empty() {
        tracing::warn!(
            "fraud rescan: no account display names — the check for somebody wearing one of \
             our own people's names cannot fire."
        );
    }
    if policy.org_names.is_empty() {
        tracing::warn!(
            "fraud rescan: MAILRS_ORG_NAMES is empty — the impersonation check cannot fire, \
             so this sweep sees only the mailer fingerprint. Set it to the same value the \
             receiver has."
        );
    }
    policy
}

/// A comma-separated environment variable, or nothing.
///
/// **The receiver is a different container**, and the sentence that
/// used to be here said otherwise — "this is the same process the
/// receiver's policy is configured for". It is not, and on production
/// it never was: `MAILRS_ORG_NAMES` was set on the receiver only, so
/// every sweep this process ran had an empty org-name list and the
/// impersonation rule could not fire. Half the checks, on the only
/// lane that runs the sweep, with nothing to say so — the count came
/// back plausible because the other rule still worked.
///
/// Found on 2026-08-28 by running the sweep against a copy of
/// production and getting a number eight times too large, because the
/// copy had been given a *wider* policy than production has. The
/// compose file now gives this process the receiver's block verbatim.
///
/// Empty is therefore worth noticing: [`policy_from_env`] logs when a
/// sweep is about to run with no org names, because "found nothing"
/// and "could not look" are different answers.
pub(super) fn csv_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// This deployment's own `authserv-id` — the name its receiver writes
/// into `Authentication-Results:`.
///
/// It is how a sweep tells a message that arrived from a stranger from
/// one this deployment's own people submitted: the receiver stamps
/// that header on the inbound path only, and that path runs only for
/// sessions that did not authenticate. `None` when `MAILRS_HOSTNAME`
/// was not given to this process — said out loud, because the rules
/// that ask then decline and a silent decline reads as "nothing
/// found".
pub(super) fn our_authserv_id() -> Option<String> {
    let host = std::env::var("MAILRS_HOSTNAME")
        .ok()
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty());
    if host.is_none() {
        tracing::warn!(
            "fraud rescan: MAILRS_HOSTNAME is unset — this sweep cannot tell mail that arrived \
             from a stranger from mail this deployment's own people submitted, so \
             `claims-our-domain` cannot fire on any of it"
        );
    }
    host
}

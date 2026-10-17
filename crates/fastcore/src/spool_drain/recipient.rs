//! One resolved local recipient of a spooled message: report side
//! effects, sieve, and the maildir write plus ingest. Shared by the
//! envelope's own recipients and by the members of an email group.

use std::sync::Arc;

use mailrs_core::spool::SpoolEnvelope;

use super::deliver::{
    canonical_recipient, deliver, enqueue_outbound, enqueue_redirect, has_maildir,
    provision_if_account,
};
use super::reported_to;
use crate::FastcoreState;

/// The spooled message being delivered.
pub(super) struct Inbound<'a> {
    pub state: &'a Arc<FastcoreState>,
    pub maildir_root: &'a str,
    pub filename: &'a str,
    pub body: &'a [u8],
    pub env: &'a SpoolEnvelope,
}

/// The local mailbox `fwd` delivers to: its own maildir, an account that
/// gets its maildir now, or the alias target that has either.
pub(super) fn resolve_local(
    state: &Arc<FastcoreState>,
    maildir_root: &str,
    fwd: &str,
) -> Option<String> {
    if has_maildir(maildir_root, fwd) || provision_if_account(state, maildir_root, fwd) {
        // an account that was never provisioned a maildir (fresh
        // add_account) gets the skeleton so its very first mail lands
        return Some(fwd.to_string());
    }
    let a = canonical_recipient(&state.alias_store.resolve(fwd).ok().flatten()?);
    if has_maildir(maildir_root, &a) || provision_if_account(state, maildir_root, &a) {
        tracing::info!(orig = %fwd, aliased = %a, "spool alias resolved");
        Some(a)
    } else {
        None
    }
}

impl Inbound<'_> {
    /// Deliver to `addr`, the mailbox `fwd` resolved to. Returns whether
    /// the message was consumed for this recipient (delivered, filed,
    /// redirected, or deliberately discarded / rejected by sieve).
    pub(super) fn deliver_resolved(&self, fwd: &str, addr: &str) -> bool {
        let (state, maildir_root, filename, body, env) = (
            self.state,
            self.maildir_root,
            self.filename,
            self.body,
            self.env,
        );
        // DMARC aggregate reports, feedback-loop complaints and TLS-RPT
        // reports to their collector mailboxes are parsed here as a side
        // effect; the message still lands in the mailbox as ordinary mail.
        crate::dmarc_ingest::maybe_ingest(reported_to(fwd, addr), body);
        crate::fbl::maybe_record_complaint(maildir_root, reported_to(fwd, addr), body);
        crate::tlsrpt_ingest::maybe_ingest(reported_to(fwd, addr), body);
        let outcome = crate::sieve_apply::decide(addr, body, Some(&env.reverse_path));
        let ingest = |blob_ref: &str, folder: &str| {
            crate::ingest_delivered_file(
                state,
                addr,
                blob_ref,
                body,
                folder,
                env.fraud_verdict.as_deref(),
            );
        };
        let inbox = || match deliver(maildir_root, addr, "", filename, body) {
            Ok(true) => {
                ingest(filename, &env.target_folder);
                true
            }
            Ok(false) => false,
            Err(e) => {
                tracing::warn!(fwd = %addr, error = %e, "spool deliver");
                false
            }
        };
        // vacation fires only after a successful LOCAL delivery
        let (consumed, delivered_locally) = match outcome.decision {
            crate::sieve_apply::Decision::Discard => {
                tracing::info!(recipient = %addr, "sieve: discard");
                (true, false)
            }
            crate::sieve_apply::Decision::Reject(reason) => {
                self.reject_dsn(addr, &reason);
                (true, false)
            }
            crate::sieve_apply::Decision::Redirect(target) => {
                match enqueue_redirect(addr, &target, body, &env.reverse_path) {
                    Ok(()) => {
                        tracing::info!(recipient = %addr, %target, "sieve: redirect");
                        (true, false)
                    }
                    Err(e) => {
                        tracing::warn!(recipient = %addr, %target, error = %e,
                            "sieve: redirect enqueue failed; falling back to Keep");
                        let ok = inbox();
                        (ok, ok)
                    }
                }
            }
            crate::sieve_apply::Decision::FileInto(folder) => {
                let subfolder = crate::sieve_apply::maildir_subfolder(&folder);
                match deliver(maildir_root, addr, &subfolder, filename, body) {
                    Ok(true) => {
                        tracing::info!(recipient = %addr, %subfolder, "sieve: fileinto");
                        ingest(&format!("{subfolder}/{filename}"), &folder);
                        (true, true)
                    }
                    other => {
                        tracing::warn!(recipient = %addr, %subfolder, result = ?other,
                            "sieve: fileinto failed; falling back to INBOX");
                        let ok = inbox();
                        (ok, ok)
                    }
                }
            }
            crate::sieve_apply::Decision::Keep => {
                let ok = inbox();
                (ok, ok)
            }
        };
        if delivered_locally && let Some(vac) = outcome.vacation {
            crate::sieve_apply::maybe_vacation_reply(addr, &env.reverse_path, body, &vac);
        }
        consumed
    }

    /// Backscatter guard: a DSN only when the receiver's antispam verdict
    /// routed to INBOX (auth-scored OK proxy — the envelope carries no raw
    /// SPF/DKIM result) AND the sender is a real address. Anything else is
    /// silently consumed.
    fn reject_dsn(&self, addr: &str, reason: &str) {
        let env = self.env;
        let allow = env.target_folder.eq_ignore_ascii_case("INBOX")
            && !crate::bounce::suppress_bounce(&env.reverse_path);
        if !allow {
            tracing::info!(recipient = %addr, "sieve: reject suppressed (backscatter guard)");
            return;
        }
        let (reporting_mta, from_domain) = crate::bounce::dsn_identity();
        let dsn = crate::bounce::compose_dsn(
            &reporting_mta,
            &from_domain,
            &env.reverse_path,
            addr,
            "5.7.1",
            &format!("550 5.7.1 {reason}"),
            self.body,
        );
        match enqueue_outbound(&env.reverse_path, "", &dsn) {
            Ok(()) => tracing::info!(recipient = %addr, "sieve: reject DSN queued"),
            Err(e) => tracing::warn!(recipient = %addr, error = %e,
                "sieve: reject DSN enqueue failed; message discarded"),
        }
    }
}

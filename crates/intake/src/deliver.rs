//! A copy that reached one of a core's mailboxes: the report hooks, the
//! recipient's sieve script, and the maildir write the core then indexes.

use crate::drain::{Core, Inbound};
use crate::maildir::{deliver, maildir_subfolder};
use crate::outbound::{enqueue_outbound, enqueue_redirect};
use crate::sieve::{self, Decision};

impl<C: Core + ?Sized> Inbound<'_, C> {
    /// Deliver to `addr`, the mailbox `fwd` resolved to. Returns whether
    /// the message was consumed for this recipient (delivered, filed,
    /// redirected, or deliberately discarded / rejected by sieve).
    pub fn deliver_resolved(&self, fwd: &str, addr: &str) -> bool {
        crate::reports::ingest(self.core.maildir_root(), fwd, addr, self.body);
        self.place(addr)
    }

    /// `addr`'s sieve script, then the maildir write it decides on.
    pub(crate) fn place(&self, addr: &str) -> bool {
        let (body, env) = (self.body, self.env);
        let outcome = sieve::decide(addr, body, Some(&env.reverse_path));
        // vacation fires only after a successful local delivery
        let (consumed, delivered_locally) = match outcome.decision {
            Decision::Discard => {
                tracing::info!(recipient = %addr, "sieve: discard");
                (true, false)
            }
            Decision::Reject(reason) => {
                self.reject_dsn(addr, &reason);
                (true, false)
            }
            // a redirect the forwarding rules refuse is not made: the copy
            // stays here, as if the script had kept it
            Decision::Redirect(target) if !self.may_redirect(addr, &target) => {
                tracing::warn!(recipient = %addr, %target,
                    "sieve: redirect refused (not ours, not the verified forwarding address); kept");
                let ok = self.keep(addr);
                (ok, ok)
            }
            // a redirect keeps no copy here
            Decision::Redirect(target) => {
                match enqueue_redirect(addr, &target, body, &env.reverse_path) {
                    Ok(()) => {
                        tracing::info!(recipient = %addr, %target, "sieve: redirect");
                        (true, false)
                    }
                    Err(e) => {
                        tracing::warn!(recipient = %addr, %target, error = %e,
                            "sieve: redirect enqueue failed; falling back to Keep");
                        let ok = self.keep(addr);
                        (ok, ok)
                    }
                }
            }
            // the script filed it: forwarded as well, the filed copy kept
            Decision::FileInto(folder) => {
                self.forward_by_settings(addr);
                let ok = self.file_into(addr, &folder);
                (ok, ok)
            }
            Decision::Keep => match self.forward_by_settings(addr) {
                true => {
                    let ok = self.keep(addr);
                    (ok, ok)
                }
                // forwarded without a copy here, as a redirect is
                false => (true, false),
            },
        };
        if delivered_locally {
            match outcome.vacation {
                Some(vac) => self.vacation_by_script(addr, vac),
                None => self.vacation_by_settings(addr, now()),
            }
        }
        consumed
    }

    /// Whether the forwarding rules let `addr`'s script redirect to
    /// `target`. A store that cannot be read refuses.
    fn may_redirect(&self, addr: &str, target: &str) -> bool {
        let Some(mut conn) = self.core.net_conn() else {
            return false;
        };
        let domains = self.core.local_domains();
        match crate::sieve_policy::may_redirect(&mut conn, addr, target, &domains) {
            Ok(ok) => ok,
            Err(e) => {
                tracing::warn!(recipient = %addr, error = %e, "sieve: forwarding settings unreadable");
                false
            }
        }
    }

    /// A script's vacation reply, under the rules the one set on the web
    /// keeps: none to Junk or held mail, and sent only as the user or an
    /// address it may send as.
    fn vacation_by_script(&self, addr: &str, mut vac: sieve::Vacation) {
        if !self.settings_apply(addr) {
            return;
        }
        if let Some(from) = vac.from.as_deref()
            && !from.trim().eq_ignore_ascii_case(addr)
            && !self.core.may_send_as(addr, from.trim())
        {
            tracing::info!(recipient = %addr, "vacation: :from is not the user's to send as; replying as the user");
            vac.from = None;
        }
        sieve::maybe_vacation_reply(addr, &self.env.reverse_path, self.body, &vac);
    }

    /// The copy in INBOX, filed where the receiver's verdict put it.
    fn keep(&self, addr: &str) -> bool {
        let root = self.core.maildir_root();
        match deliver(root, addr, "", self.filename, self.body) {
            Ok(true) => self.core.ingest(
                addr,
                self.filename,
                self.body,
                &self.env.target_folder,
                self.env.fraud_verdict.as_deref(),
            ),
            Ok(false) => false,
            Err(e) => {
                tracing::warn!(fwd = %addr, error = %e, "spool deliver");
                false
            }
        }
    }

    /// The copy in the folder a sieve `fileinto` names, or INBOX when that
    /// cannot be written.
    fn file_into(&self, addr: &str, folder: &str) -> bool {
        let subfolder = maildir_subfolder(folder);
        let root = self.core.maildir_root();
        match deliver(root, addr, &subfolder, self.filename, self.body) {
            Ok(true) => {
                tracing::info!(recipient = %addr, %subfolder, "sieve: fileinto");
                self.core.ingest(
                    addr,
                    &format!("{subfolder}/{}", self.filename),
                    self.body,
                    folder,
                    self.env.fraud_verdict.as_deref(),
                )
            }
            other => {
                tracing::warn!(recipient = %addr, %subfolder, result = ?other,
                    "sieve: fileinto failed; falling back to INBOX");
                self.keep(addr)
            }
        }
    }

    /// Backscatter guard: a DSN only when the receiver's antispam verdict
    /// routed to INBOX (auth-scored OK proxy — the envelope carries no raw
    /// SPF/DKIM result) AND the sender is a real address. Anything else is
    /// silently consumed.
    fn reject_dsn(&self, addr: &str, reason: &str) {
        let env = self.env;
        let allow = env.target_folder.eq_ignore_ascii_case("INBOX")
            && !crate::dsn::suppress_bounce(&env.reverse_path);
        if !allow {
            tracing::info!(recipient = %addr, "sieve: reject suppressed (backscatter guard)");
            return;
        }
        let (reporting_mta, from_domain) = crate::dsn::dsn_identity();
        let dsn = crate::dsn::compose_dsn(
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

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

//! What the user set for themselves on the web: forwarding to an address
//! elsewhere, and a vacation reply. Done after the account's sieve script,
//! whose own redirect or vacation comes first, and only for mail that is
//! neither in Junk nor held: forwarding junk would spend this server's
//! reputation on it, and replying to it confirms the address to a spammer.

use mailrs_core_sidestate::families::{forwarding, vacation};

use crate::drain::{Core, Inbound};
use crate::outbound::enqueue_redirect;
use crate::sieve;

impl<C: Core + ?Sized> Inbound<'_, C> {
    /// Whether the user's settings may act on this copy for `addr`.
    pub(crate) fn settings_apply(&self, addr: &str) -> bool {
        let env = self.env;
        let rp = env.reverse_path.trim().trim_matches(['<', '>']);
        !env.target_folder.eq_ignore_ascii_case("Junk")
            && !held(env.fraud_verdict.as_deref())
            && !rp.is_empty()
            && !rp.eq_ignore_ascii_case(addr)
    }

    /// Forward to the verified address, if there is one. Returns whether a
    /// copy is still to be kept here.
    pub(crate) fn forward_by_settings(&self, addr: &str) -> bool {
        if !self.settings_apply(addr) {
            return true;
        }
        let Some(mut conn) = self.core.net_conn() else {
            return true;
        };
        let f = match forwarding::get(&mut conn, addr) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(recipient = %addr, error = %e, "forwarding: settings unreadable; keeping");
                return true;
            }
        };
        let Some(target) = f.target() else {
            return true;
        };
        let once = self.once("forward", addr, target);
        match enqueue_redirect(addr, target, self.body, &self.env.reverse_path, Some(&once)) {
            Ok(()) => {
                tracing::info!(recipient = %addr, %target, "forwarding: queued");
                f.keep_copy
            }
            Err(e) => {
                tracing::warn!(recipient = %addr, %target, error = %e, "forwarding: enqueue failed; keeping");
                true
            }
        }
    }

    /// The vacation reply set on the web, when it is on now.
    pub(crate) fn vacation_by_settings(&self, addr: &str, now: i64) {
        if !self.settings_apply(addr) {
            return;
        }
        let Some(mut conn) = self.core.net_conn() else {
            return;
        };
        let Ok(Some(v)) = vacation::get(&mut conn, addr) else {
            return;
        };
        if !v.active_at(now) {
            return;
        }
        let subject = match v.subject.trim().is_empty() {
            true => None,
            false => Some(v.subject.clone()),
        };
        let reply = sieve::Vacation {
            reason: v.body,
            subject,
            from: None,
            // a changed reply is a new one: senders the old one answered get it
            handle: Some(format!("settings-{}", v.version)),
            period_secs: u64::from(v.days) * 86_400,
            addresses: Vec::new(),
        };
        sieve::maybe_vacation_reply(addr, &self.env.reverse_path, self.body, &reply);
    }
}

/// Whether the receiver's verdict holds the message as fraud.
fn held(verdict: Option<&str>) -> bool {
    verdict
        .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
        .is_some_and(|v| v["quarantined"] == true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_verdict_holds_only_when_it_says_so() {
        assert!(super::held(Some(r#"{"quarantined":true,"score":9}"#)));
        assert!(!super::held(Some(r#"{"quarantined":false}"#)));
        assert!(!super::held(Some("not json")));
        assert!(!super::held(None));
    }
}

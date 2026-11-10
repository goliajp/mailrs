//! Who may make the receiver visit a sender's web root, and how often.
//!
//! The visit ([`crate::spam_lists::site_elsewhere_async`]) is an outbound
//! request the sender of a message chooses the target of. Two gates keep
//! it from being used to aim the receiver at a host: the `From` domain
//! must have authenticated the message itself
//! ([`from_domain_authenticated`]), and visits are bounded both across
//! the process and per connecting address ([`SiteVisits`]). A visit
//! refused by a limit is not made and nothing is recorded, so the
//! message reads as if the site had never been seen.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use hickory_resolver::TokioResolver;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The two bounds on visits, from `MAILRS_SITE_VISIT_MAX_CONCURRENT` and
/// `MAILRS_SITE_VISIT_PER_IP_PER_MINUTE`.
pub struct SiteVisits {
    slots: Arc<Semaphore>,
    per_ip_per_minute: u32,
    /// Visits started this minute, by connecting address. Only the
    /// current minute is kept.
    recent: Mutex<(u64, HashMap<IpAddr, u32>)>,
}

impl SiteVisits {
    pub fn new(max_concurrent: usize, per_ip_per_minute: u32) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(max_concurrent)),
            per_ip_per_minute,
            recent: Mutex::new((0, HashMap::new())),
        }
    }

    /// A slot to visit now on behalf of a message from `ip`, or `None`
    /// when every slot is taken or `ip` has used its visits for the
    /// minute. The slot is held for the visit and released when dropped.
    pub fn admit(&self, ip: IpAddr, minute: u64) -> Option<OwnedSemaphorePermit> {
        let mut recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        if recent.0 != minute {
            *recent = (minute, HashMap::new());
        }
        let used = recent.1.get(&ip).copied().unwrap_or(0);
        if used >= self.per_ip_per_minute {
            return None;
        }
        let permit = self.slots.clone().try_acquire_owned().ok()?;
        recent.1.insert(ip, used + 1);
        Some(permit)
    }
}

/// The current minute since the epoch, the window [`SiteVisits::admit`]
/// counts in.
pub fn this_minute() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 60)
        .unwrap_or(0)
}

/// Whether the message's `From` domain authenticated it: SPF passed for
/// an envelope sender at that domain, or a DKIM signature by that domain
/// verified ([`mailrs_fraud::burner_domain::speaks_for`]).
///
/// Asked before the receive pipeline has checked SPF and DKIM, and only
/// for a message already worth the visit, so the second check of those
/// few messages is the price of knowing before the verdict is made.
/// Without a resolver nothing is authenticated.
pub async fn from_domain_authenticated(
    resolver: Option<Arc<TokioResolver>>,
    ctx: &mailrs_inbound::ReceiveContext,
    from_host: &str,
) -> bool {
    use mailrs_fraud::burner_domain::speaks_for;
    let Some(resolver) = resolver else {
        return false;
    };
    let envelope_domain = ctx.sender.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
    if speaks_for(from_host, envelope_domain) {
        let spf = mailrs_spf::HickoryResolver::new((*resolver).clone());
        let input = mailrs_spf::VerifyInput {
            ip: ctx.client_ip,
            helo: ctx.ehlo_domain.clone(),
            mail_from: ctx.sender.clone(),
        };
        if mailrs_spf::verify(&spf, &input).await == mailrs_spf::SpfResult::Pass {
            return true;
        }
    }
    let signed_by_from = mailrs_inbound::dkim_signers::of(&ctx.message)
        .iter()
        .any(|d| speaks_for(from_host, d));
    if !signed_by_from {
        return false;
    }
    let dkim = mailrs_dkim::HickoryDkimResolver::new((*resolver).clone());
    mailrs_dkim::verify_all(&dkim, &ctx.message)
        .await
        .iter()
        .any(|o| o.is_pass() && speaks_for(from_host, o.domain()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, last])
    }

    #[test]
    fn an_address_gets_its_visits_for_the_minute_and_then_none() {
        let visits = SiteVisits::new(10, 2);
        let _a = visits.admit(ip(1), 100).expect("first");
        let _b = visits.admit(ip(1), 100).expect("second");
        assert!(visits.admit(ip(1), 100).is_none());
        // another address is counted apart
        assert!(visits.admit(ip(2), 100).is_some());
        // and the next minute starts over
        assert!(visits.admit(ip(1), 101).is_some());
    }

    #[test]
    fn no_more_visits_run_at_once_than_there_are_slots() {
        let visits = SiteVisits::new(2, 100);
        let a = visits.admit(ip(1), 7).expect("first slot");
        let _b = visits.admit(ip(2), 7).expect("second slot");
        assert!(visits.admit(ip(3), 7).is_none());
        drop(a);
        assert!(visits.admit(ip(3), 7).is_some());
    }

    #[test]
    fn a_refused_visit_does_not_use_up_the_addresses_count() {
        let visits = SiteVisits::new(1, 1);
        let held = visits.admit(ip(1), 3).expect("slot");
        assert!(visits.admit(ip(2), 3).is_none());
        drop(held);
        assert!(visits.admit(ip(2), 3).is_some());
    }
}

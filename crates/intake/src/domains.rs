//! Which domains are ours: `MAILRS_LOCAL_DOMAINS` together with the domain
//! store the admin Domains page writes.

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

static ENV_DOMAINS: LazyLock<Vec<String>> = LazyLock::new(|| {
    let domains: Vec<String> = std::env::var("MAILRS_LOCAL_DOMAINS")
        .unwrap_or_default()
        .split(',')
        .map(|d| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect();
    if domains.is_empty() {
        tracing::warn!(
            "MAILRS_LOCAL_DOMAINS is empty: only the domains on the admin Domains page \
             count as ours, and an authenticated submission to any other is relayed"
        );
    }
    domains
});

const CACHE_TTL: Duration = Duration::from_secs(30);

/// The env domains first, in their own order, then the store's, read from
/// the store at most every 30 seconds.
#[derive(Default)]
pub struct LocalDomainsCache {
    cached: Mutex<Option<(Instant, Arc<Vec<String>>)>>,
}

impl LocalDomainsCache {
    /// `store` lists the domain store's names. When it fails the previous
    /// answer stands, or the env domains alone before there is one.
    pub fn get(&self, store: impl FnOnce() -> std::io::Result<Vec<String>>) -> Arc<Vec<String>> {
        let mut cached = self.cached.lock().expect("local domains cache lock");
        if let Some((at, domains)) = cached.as_ref()
            && at.elapsed() < CACHE_TTL
        {
            return domains.clone();
        }
        let domains = match store() {
            Ok(names) => Arc::new(union(&ENV_DOMAINS, names.into_iter())),
            Err(e) => {
                tracing::warn!(error = %e, "domain store read failed; keeping the previous local domains");
                match cached.as_ref() {
                    Some((_, previous)) => previous.clone(),
                    None => Arc::new(ENV_DOMAINS.clone()),
                }
            }
        };
        *cached = Some((Instant::now(), domains.clone()));
        domains
    }
}

fn union(env: &[String], store: impl Iterator<Item = String>) -> Vec<String> {
    let mut out = env.to_vec();
    for d in store {
        let d = d.trim().to_ascii_lowercase();
        if !d.is_empty() && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// Whether `rcpt` (already lowercased) leaves through the outbound queue
/// instead of being delivered here. Only an authenticated session can
/// relay: the receiver answers 550 to an unauthenticated one before it
/// spools anything addressed outside our domains.
pub fn relays_outward(is_authenticated: bool, rcpt: &str, local_domains: &[String]) -> bool {
    let Some((_, domain)) = rcpt.rsplit_once('@') else {
        return false;
    };
    is_authenticated && !local_domains.iter().any(|d| d == domain)
}

#[cfg(test)]
mod tests {
    use super::relays_outward;

    fn ours() -> Vec<String> {
        vec!["golia.jp".into(), "golia.ai".into()]
    }

    #[test]
    fn an_authenticated_submission_to_another_domain_is_relayed() {
        assert!(relays_outward(true, "someone@example.com", &ours()));
    }

    #[test]
    fn a_recipient_we_host_is_delivered_here_even_when_authenticated() {
        assert!(!relays_outward(true, "lihao@golia.jp", &ours()));
        assert!(!relays_outward(true, "sales@golia.ai", &ours()));
    }

    /// The receiver refuses these, but a spool file is a file: one that
    /// says unauthenticated never becomes outbound mail.
    #[test]
    fn an_unauthenticated_envelope_is_never_relayed() {
        assert!(!relays_outward(false, "someone@example.com", &ours()));
    }

    /// A subdomain of ours is not ours unless it is listed.
    #[test]
    fn only_the_exact_domain_counts_as_ours() {
        assert!(relays_outward(true, "a@mail.golia.jp", &ours()));
        assert!(relays_outward(true, "a@notgolia.jp", &ours()));
    }

    #[test]
    fn an_address_without_a_domain_is_not_relayed() {
        assert!(!relays_outward(true, "postmaster", &ours()));
    }
}

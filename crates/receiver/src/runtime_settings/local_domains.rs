//! The domains the receiver accepts mail for: `MAILRS_LOCAL_DOMAINS`
//! together with the domain store the admin Domains page writes.
//!
//! The store lives in fastcore's embedded kevy, which only fastcore can
//! open, so the receiver lists it over core RPC at boot and every minute.
//! A failed fetch keeps the previous set; it is never cleared.
//!
//! An empty set still means "every domain is local"
//! (`smtp_session::address::is_local_domain`), and now that takes both
//! halves to be empty. A deployment without `MAILRS_LOCAL_DOMAINS` that
//! adds its first domain on the Domains page therefore stops treating
//! every other domain as local within a minute: from then on the page is
//! the list, and an unauthenticated RCPT to any other domain is refused
//! as relaying. Production sets the variable, so it only gains domains.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use mailrs_core_api::client::Client;
use mailrs_core_api::error::CoreApiError;

/// Cheap to clone; every clone sees the same refreshed set.
#[derive(Clone)]
pub struct LocalDomains {
    inner: Arc<Inner>,
}

struct Inner {
    env: Vec<String>,
    current: RwLock<Arc<Vec<String>>>,
}

impl LocalDomains {
    pub fn new(env: Vec<String>) -> Self {
        let env = union(Vec::new(), env);
        Self {
            inner: Arc::new(Inner {
                current: RwLock::new(Arc::new(env.clone())),
                env,
            }),
        }
    }

    /// Lowercased, environment domains first.
    pub fn current(&self) -> Arc<Vec<String>> {
        self.inner
            .current
            .read()
            .expect("local domains lock")
            .clone()
    }

    pub fn as_set(&self) -> std::collections::HashSet<String> {
        self.current().iter().cloned().collect()
    }

    /// Replace the store half of the set; true when the set changed.
    pub fn set_store(&self, store: impl IntoIterator<Item = String>) -> bool {
        let next = union(self.inner.env.clone(), store);
        let mut current = self.inner.current.write().expect("local domains lock");
        if **current == next {
            return false;
        }
        *current = Arc::new(next);
        true
    }
}

fn union(mut into: Vec<String>, more: impl IntoIterator<Item = String>) -> Vec<String> {
    for d in more {
        let d = d.trim().to_ascii_lowercase();
        if !d.is_empty() && !into.contains(&d) {
            into.push(d);
        }
    }
    into
}

/// Fetch the domain store once; true when the set changed.
pub async fn refresh_local_domains(
    domains: &LocalDomains,
    core: &Client,
) -> Result<bool, CoreApiError> {
    let listed = core.list_domains().await?;
    Ok(domains.set_store(listed.items.into_iter().map(|d| d.name)))
}

/// Fetch the domain store once and log the outcome.
///
/// The receiver awaits this before it listens: a domain that is on the
/// page but not yet in the set gets a permanent 550 for relaying, so the
/// first minute after a restart must not be spent without it.
pub async fn refresh_and_log(domains: &LocalDomains, core: &Client) {
    match refresh_local_domains(domains, core).await {
        Ok(true) => tracing::info!(domains = ?domains.current(), "local domains changed"),
        Ok(false) => {}
        Err(e) => tracing::warn!(
            error = %e,
            "domain store fetch failed; keeping the previous local domains"
        ),
    }
}

/// Refresh every `every`, after the boot fetch.
///
/// Each tick is one GET of a list of a few dozen names. It cannot be made
/// incremental, because fastcore keeps no change feed for the domain
/// store, and backing off would delay the one thing this is for: a domain
/// added on the page being received within a minute.
pub fn spawn_local_domains_refresh(domains: LocalDomains, core: Client, every: Duration) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(every).await;
            refresh_and_log(&domains, &core).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::LocalDomains;

    #[test]
    fn the_store_adds_to_the_environment_and_never_removes_from_it() {
        let d = LocalDomains::new(vec!["golia.jp".into()]);
        assert!(d.set_store(["New.Example".to_string(), "golia.jp".into()]));
        assert_eq!(*d.current(), ["golia.jp", "new.example"]);
        assert!(d.set_store(Vec::new()));
        assert_eq!(*d.current(), ["golia.jp"]);
    }

    #[test]
    fn an_unchanged_store_is_not_a_change() {
        let d = LocalDomains::new(Vec::new());
        assert!(d.set_store(["a.example".to_string()]));
        assert!(!d.set_store(["a.example".to_string()]));
    }
}

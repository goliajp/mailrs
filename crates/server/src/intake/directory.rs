//! This core's recipients as the shared routing order asks about them.
//!
//! The shared drain is synchronous, as the kevy core is; the SQL store is
//! not. Every method here blocks on the runtime handle it was built with,
//! so it must be called from a blocking thread (`spawn_blocking`), never
//! from a runtime worker.

use std::io;
use std::sync::Arc;

use mailrs_core_sidestate::families::email_groups::Group;
use mailrs_intake::domains::LocalDomainsCache;
use mailrs_intake::route::{Directory, Route, route};

use crate::domain_store::DomainStore;

pub(crate) struct PgDirectory {
    pub domain: Arc<DomainStore>,
    pub rt: tokio::runtime::Handle,
    pub maildir_root: String,
    pub net_url: Option<String>,
    pub local_domains: LocalDomainsCache,
}

impl PgDirectory {
    /// Built on a runtime thread: it keeps that runtime's handle.
    pub fn new(domain: Arc<DomainStore>, maildir_root: String, net_url: Option<String>) -> Self {
        Self {
            domain,
            rt: tokio::runtime::Handle::current(),
            maildir_root,
            net_url,
            local_domains: LocalDomainsCache::default(),
        }
    }

    /// Where `raw`'s mail goes, asked from async code.
    pub async fn route(self: Arc<Self>, raw: String) -> io::Result<Route> {
        tokio::task::spawn_blocking(move || {
            let fwd = mailrs_intake::maildir::canonical_recipient(raw.trim());
            route(&*self, raw.trim(), &fwd)
        })
        .await
        .map_err(io::Error::other)?
    }
}

impl Directory for PgDirectory {
    fn maildir_root(&self) -> &str {
        &self.maildir_root
    }

    fn is_account(&self, addr: &str) -> io::Result<bool> {
        self.rt
            .block_on(self.domain.is_active_account(addr))
            .map_err(io::Error::other)
    }

    fn alias(&self, addr: &str) -> io::Result<Option<String>> {
        self.rt
            .block_on(self.domain.alias_for(addr))
            .map_err(io::Error::other)
    }

    fn group(&self, addr: &str) -> io::Result<Option<Group>> {
        self.rt
            .block_on(self.domain.email_group(self.net_url.as_deref(), addr))
            .map_err(io::Error::other)
    }

    fn local_domains(&self) -> Arc<Vec<String>> {
        self.local_domains.get(|| {
            self.rt
                .block_on(self.domain.domain_names())
                .map_err(io::Error::other)
        })
    }
}

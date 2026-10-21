//! This core as the shared drain sees it: who its recipients are, read
//! from the embedded store and the network kevy.

use std::io;
use std::sync::Arc;

use mailrs_core_sidestate::families::email_groups::{Group, find_by_address};
use mailrs_intake::route::Directory;

use crate::FastcoreState;

pub(crate) struct Fastcore<'a> {
    pub state: &'a Arc<FastcoreState>,
    pub maildir_root: &'a str,
}

impl Directory for Fastcore<'_> {
    fn maildir_root(&self) -> &str {
        self.maildir_root
    }

    fn is_account(&self, addr: &str) -> io::Result<bool> {
        Ok(self.state.mailbox.get_account_blob(addr)?.is_some())
    }

    fn alias(&self, addr: &str) -> io::Result<Option<String>> {
        self.state.alias_store.resolve(addr)
    }

    /// No network kevy configured means no groups; one that is configured
    /// and cannot be reached is an error.
    fn group(&self, addr: &str) -> io::Result<Option<Group>> {
        let Some(url) = self.state.net_url.as_deref() else {
            return Ok(None);
        };
        let mut conn = kevy_client::Connection::connect(url).map_err(io::Error::other)?;
        find_by_address(&mut conn, addr)
    }

    fn local_domains(&self) -> Arc<Vec<String>> {
        super::relay::local_domains(self.state)
    }
}

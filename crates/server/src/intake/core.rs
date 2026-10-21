//! This core as the shared drain sees it.

use std::sync::Arc;

use mailrs_core_sidestate::families::email_groups::Group;
use mailrs_intake::Core;
use mailrs_intake::route::Directory;
use mailrs_mailbox::PgMailboxStore;

use super::PgDirectory;
use crate::event_bus::EventBus;

pub(crate) struct PgCore {
    pub dir: PgDirectory,
    pub mailbox: Arc<PgMailboxStore>,
    /// the in-process bus IMAP IDLE and the cache listen on
    pub event_bus: EventBus,
}

impl Directory for PgCore {
    fn maildir_root(&self) -> &str {
        self.dir.maildir_root()
    }
    fn is_account(&self, addr: &str) -> std::io::Result<bool> {
        self.dir.is_account(addr)
    }
    fn alias(&self, addr: &str) -> std::io::Result<Option<String>> {
        self.dir.alias(addr)
    }
    fn group(&self, addr: &str) -> std::io::Result<Option<Group>> {
        self.dir.group(addr)
    }
    fn local_domains(&self) -> Arc<Vec<String>> {
        self.dir.local_domains()
    }
}

impl Core for PgCore {
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        kevy_client::Connection::connect(self.dir.net_url.as_deref()?).ok()
    }

    fn ingest(
        &self,
        addr: &str,
        blob_ref: &str,
        body: &[u8],
        folder: &str,
        fraud_verdict: Option<&str>,
    ) -> bool {
        self.index(addr, blob_ref, body, folder, fraud_verdict)
    }
}

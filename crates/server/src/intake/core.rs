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
    fn mask(&self, addr: &str) -> std::io::Result<Option<Option<String>>> {
        self.dir.mask(addr)
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

    fn tag_shared(
        &self,
        addr: &str,
        message_id: &str,
        tag: &str,
        assignee: &str,
    ) -> Option<String> {
        let rt = &self.dir.rt;
        let tid = rt
            .block_on(self.mailbox.find_thread_id_by_message_id(addr, message_id))
            .ok()
            .flatten()?;
        match rt.block_on(self.mailbox.set_shared_copy(addr, &tid, tag, assignee)) {
            Ok(()) => Some(tid),
            Err(e) => {
                tracing::warn!(%addr, %tag, error = %e, "team inbox: copy not tagged");
                None
            }
        }
    }

    /// Asked from the drain's blocking thread, as every method here is.
    fn may_send_as(&self, user: &str, from: &str) -> bool {
        let perms =
            self.dir
                .rt
                .block_on(crate::core_rpc::handlers::admin::effective_permissions_of(
                    &self.dir.domain,
                    self.dir.net_url.as_deref(),
                    user,
                ));
        perms.is_ok_and(|p| p.may_send_as(from))
    }

    fn file_submitted(
        &self,
        user: &str,
        body: &[u8],
        recipients: &[String],
    ) -> std::io::Result<()> {
        mailrs_intake::sent::file_submitted(self, user, body, recipients).map(|_| ())
    }
}

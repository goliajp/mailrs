//! A copy that reached one of this core's mailboxes.

use std::sync::Arc;

use mailrs_core_sidestate::families::email_groups::Group;
use mailrs_intake::route::Directory;
use mailrs_intake::{Core, Inbound};
use mailrs_mailbox::PgMailboxStore;
use mailrs_outbound_queue::{Notifier, QueueStore};
use mailrs_receiver::AccountStore;
use mailrs_receiver::smtp_session::DeliveryDeps;
use mailrs_receiver::smtp_session::delivery::apply_sieve_actions;

use super::PgDirectory;
use crate::event_bus::EventBus;

/// What delivering into this store needs beside the directory.
pub(crate) struct Delivery {
    pub account_store: Option<Arc<dyn AccountStore>>,
    pub outbound_enqueue: Option<Arc<dyn QueueStore>>,
    pub queue_notifier: Option<Arc<dyn Notifier>>,
    pub event_bus: EventBus,
    pub hostname: String,
    pub srs_secret: Option<String>,
}

pub(crate) struct PgCore {
    pub dir: PgDirectory,
    pub mailbox: Arc<PgMailboxStore>,
    pub delivery: Delivery,
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

    fn deliver_resolved(&self, msg: &Inbound<'_, Self>, _fwd: &str, addr: &str) -> bool {
        let d = &self.delivery;
        let local_domains = self.dir.local_domains();
        let deps = DeliveryDeps {
            local_domains: &local_domains,
            account_store: &d.account_store,
            outbound_enqueue: &d.outbound_enqueue,
            queue_notifier: &d.queue_notifier,
            event_bus: &d.event_bus,
            hostname: &d.hostname,
            srs_secret: &d.srs_secret,
        };
        let (rcpt_folder, skip) = self.dir.rt.block_on(apply_sieve_actions(
            addr,
            &msg.env.target_folder,
            &msg.env.reverse_path,
            msg.body,
            &deps,
        ));
        if skip {
            return true;
        }
        let subfolder = match rcpt_folder.as_str() {
            "INBOX" | "Junk" => String::new(),
            other => mailrs_intake::maildir::maildir_subfolder(other),
        };
        match mailrs_intake::maildir::deliver(
            self.maildir_root(),
            addr,
            &subfolder,
            msg.filename,
            msg.body,
        ) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(e) => {
                tracing::warn!(fwd = %addr, error = %e, "spool deliver");
                return false;
            }
        }
        let blob_ref = match subfolder.is_empty() {
            true => msg.filename.to_string(),
            false => format!("{subfolder}/{}", msg.filename),
        };
        self.ingest(
            addr,
            &blob_ref,
            msg.body,
            &rcpt_folder,
            msg.env.fraud_verdict.as_deref(),
        )
    }
}

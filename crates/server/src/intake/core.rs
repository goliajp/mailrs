//! A copy that reached one of this core's mailboxes.

use std::sync::Arc;

use mailrs_core_sidestate::families::email_groups::Group;
use mailrs_intake::route::Directory;
use mailrs_intake::{Core, Inbound};
use mailrs_outbound_queue::{Notifier, QueueStore};
use mailrs_receiver::AccountStore;
use mailrs_receiver::smtp_session::DeliveryDeps;
use mailrs_receiver::smtp_session::delivered::{DeliveredMessage, ProcessTx};
use mailrs_receiver::smtp_session::delivery::apply_sieve_actions;

use super::PgDirectory;
use crate::event_bus::EventBus;

/// What delivering into this store needs beside the directory.
pub(crate) struct Delivery {
    /// the post-delivery consumer that indexes each delivered file
    pub process_tx: ProcessTx,
    pub account_store: Option<Arc<dyn AccountStore>>,
    pub outbound_enqueue: Option<Arc<dyn QueueStore>>,
    pub queue_notifier: Option<Arc<dyn Notifier>>,
    pub event_bus: EventBus,
    pub hostname: String,
    pub srs_secret: Option<String>,
}

pub(crate) struct PgCore {
    pub dir: PgDirectory,
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
        match mailrs_intake::maildir::deliver(self.maildir_root(), addr, "", msg.filename, msg.body)
        {
            Ok(true) => {}
            Ok(false) => return false,
            Err(e) => {
                tracing::warn!(fwd = %addr, error = %e, "spool deliver");
                return false;
            }
        }
        let full_message = Arc::new(msg.body.to_vec());
        let delivered = DeliveredMessage {
            maildir_id: msg.filename.to_string(),
            user: addr.to_string(),
            rcpt: addr.to_string(),
            rcpt_folder,
            reverse_path: msg.env.reverse_path.clone(),
            msg_message_id: mailrs_mailbox::threading::extract_message_id(&full_message),
            msg_in_reply_to: mailrs_mailbox::threading::extract_in_reply_to(&full_message),
            msg_size: full_message.len(),
            full_message,
        };
        d.process_tx.blocking_send(delivered).is_ok()
    }
}

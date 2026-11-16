//! The embedded store's part of purging a deleted account or group: its
//! conversations, its keys and the reply claims of a group's
//! conversations. The flow, the routes and the retention task are
//! `mailrs_core_routes::purge`.

use std::io;
use std::sync::Arc;

use kevy_client::Connection;
use mailrs_core_api::method::admin::PurgeCount;
use mailrs_core_routes::store::{PurgeStore, StoreError, StoreResult};
use mailrs_core_sidestate::families::account_purge as purge;
use purge::registry::{Store, spellings};

use crate::FastcoreState;

fn mail_conn(state: &FastcoreState) -> Connection {
    Connection::Embedded(Box::new(state.mailbox.store_ref().clone()))
}

/// `f` against the embedded store on a blocking thread: an account's
/// conversations can be many.
async fn blocking<T: Send + 'static>(
    state: &Arc<FastcoreState>,
    f: impl FnOnce(&FastcoreState) -> io::Result<T> + Send + 'static,
) -> StoreResult<T> {
    let st = state.clone();
    tokio::task::spawn_blocking(move || f(&st))
        .await
        .map_err(|e| StoreError(e.to_string()))?
        .map_err(StoreError::from)
}

/// Conversation by conversation first: a conversation's shared records
/// stay while another owner holds it, and go with the last. Deleting a
/// conversation takes the reader's ledger rows, so the messages read are
/// noted first, for their fingerprints, which go with the last reader.
fn remove(state: &FastcoreState, address: &str) -> io::Result<()> {
    let mut mail = mail_conn(state);
    for spelling in spellings(address) {
        let read = purge::last_reader::messages_read_by(&mut mail, &spelling)?;
        for tid in state.mailbox.all_thread_ids_for_user(&spelling)? {
            state.mailbox.delete_thread(&spelling, &tid)?;
        }
        for mid in read {
            let mid = String::from_utf8_lossy(&mid);
            state.mailbox.forget_template_if_unread(&mid)?;
        }
    }
    purge::remove_store(&mut mail, Store::Mail, address)
}

impl PurgeStore for FastcoreState {
    async fn count_mail(self: &Arc<Self>, address: &str) -> StoreResult<Vec<PurgeCount>> {
        let a = address.to_string();
        blocking(self, move |st| {
            purge::count_store(&mut mail_conn(st), Store::Mail, &a)
        })
        .await
    }

    async fn remove_mail(self: &Arc<Self>, address: &str) -> StoreResult<()> {
        let a = address.to_string();
        blocking(self, move |st| remove(st, &a)).await
    }

    /// Named `mail:<family>`, apart from the shared store's.
    async fn mail_unregistered(self: &Arc<Self>, address: &str) -> StoreResult<Vec<PurgeCount>> {
        let a = address.to_string();
        blocking(self, move |st| {
            let mut out = purge::unregistered(&mut mail_conn(st), &a)?;
            for p in &mut out {
                p.name = format!("mail:{}", p.name);
            }
            Ok(out)
        })
        .await
    }

    async fn purge_group_replies(
        self: &Arc<Self>,
        gid: i64,
        dry_run: bool,
    ) -> StoreResult<PurgeCount> {
        blocking(self, move |st| {
            Ok(PurgeCount {
                name: mailrs_mailbox_kevy::SHARED_REPLY_FAMILY.to_string(),
                count: st.mailbox.purge_group_replies(gid, dry_run)?,
            })
        })
        .await
    }
}

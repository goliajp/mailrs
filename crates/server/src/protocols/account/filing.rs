//! The bodies behind APPEND, COPY, MOVE and EXPUNGE: each row has a body of
//! its own, in its mailbox's folder (`crate::body`).

use mailrs_mail_protocols::backend::{Account, Error, Filed, Message, Result};
use mailrs_mailbox::Mailbox;

use super::{PgAccount, filed};

impl PgAccount {
    /// `bytes` as a new body in `to`'s folder, indexed there with `flags`.
    pub(super) async fn deliver(&self, to: &Mailbox, bytes: &[u8], flags: u32) -> Result<u32> {
        let store = self.message_store.as_ref();
        let blob =
            crate::body::deliver(store, &self.maildir_root, &self.user, &to.name, bytes).await?;
        let now = chrono::Utc::now().timestamp();
        self.mailbox
            .index_delivered(&self.user, &to.name, &blob, bytes, flags, now)
            .await
            .map_err(Error::Store)
    }

    /// A body of its own for `m` in `to`'s folder, then its row there, with
    /// `m`'s flags; `then_delete` drops the source row as well (MOVE).
    pub(super) async fn file(
        &self,
        from: &Mailbox,
        m: &Message<String>,
        to: &Mailbox,
        then_delete: bool,
    ) -> Result<Filed> {
        let store = self.message_store.as_ref();
        let bytes = self
            .read(m)
            .await?
            .ok_or_else(|| Error::Store(format!("message {} has no body", m.uid)))?;
        let blob =
            crate::body::deliver(store, &self.maildir_root, &self.user, &to.name, &bytes).await?;
        let row = match then_delete {
            true => {
                self.mailbox
                    .move_message(&self.user, from.id, m.uid, &to.name, Some(&blob))
                    .await
            }
            false => {
                self.mailbox
                    .copy_message(&self.user, from.id, m.uid, &to.name, Some(&blob))
                    .await
            }
        };
        let uid = match row {
            Ok(Some(uid)) => uid,
            // the new body would be a file no row names
            other => {
                crate::body::remove(store, &self.maildir_root, &self.user, &blob).await?;
                let e = other.err().map_or("gone".to_string(), |e| e.to_string());
                return Err(Error::Store(format!("message {}: {e}", m.uid)));
            }
        };
        if then_delete {
            crate::body::remove(store, &self.maildir_root, &self.user, &m.key).await?;
        }
        filed(to, Some(uid), m)
    }

    /// Remove the bodies of those of `doomed` whose uid is in `removed`.
    pub(super) async fn remove_bodies(
        &self,
        doomed: &[Message<String>],
        removed: &[u32],
    ) -> Result<()> {
        let store = self.message_store.as_ref();
        for m in doomed.iter().filter(|m| removed.contains(&m.uid)) {
            crate::body::remove(store, &self.maildir_root, &self.user, &m.key).await?;
        }
        Ok(())
    }
}

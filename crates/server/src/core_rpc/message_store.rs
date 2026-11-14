//! The SQL store's side of reading a user's messages, for the shared
//! routes. Rows become the wire shape here, with the facts the message
//! reads do not select (attachment count, invitation method) filled in.

use mailrs_core_api::method::message::MessageWire;
use mailrs_core_routes::store::{MessageReads, StoreError, StoreResult};

use super::CoreRpcState;

fn failed(e: sqlx::Error) -> StoreError {
    StoreError(e.to_string())
}

impl CoreRpcState {
    /// The attachment count each message was stored with and the invitation
    /// it carries, which the rows the message reads come from do not select.
    async fn fill_message_facts(&self, items: &mut [MessageWire]) -> StoreResult<()> {
        let ids: Vec<i64> = items.iter().map(|w| w.id).collect();
        let counts = self.mailbox.attachment_counts(&ids).await.map_err(failed)?;
        let mut invites = self.mailbox.invite_methods(&ids).await.map_err(failed)?;
        for w in items {
            w.attachments = counts.get(&w.id).copied();
            w.invite_method = invites.remove(&w.id).unwrap_or_default();
        }
        Ok(())
    }

    async fn one(
        &self,
        row: Option<mailrs_mailbox::types::MessageMeta>,
    ) -> StoreResult<Option<MessageWire>> {
        let Some(row) = row else {
            return Ok(None);
        };
        let mut w: MessageWire = (&row).into();
        self.fill_message_facts(std::slice::from_mut(&mut w))
            .await?;
        Ok(Some(w))
    }
}

impl MessageReads for CoreRpcState {
    async fn thread_messages(&self, user: &str, thread_id: &str) -> StoreResult<Vec<MessageWire>> {
        let rows = self
            .mailbox
            .list_thread_messages(user, thread_id, None)
            .await
            .map_err(failed)?;
        let mut items: Vec<MessageWire> = rows.iter().map(Into::into).collect();
        self.fill_message_facts(&mut items).await?;
        Ok(items)
    }

    async fn message_by_uid(&self, user: &str, uid: u32) -> StoreResult<Option<MessageWire>> {
        // a uid is the user's, whichever mailbox the message sits in
        let Some(meta) = self
            .mailbox
            .find_message_by_uid(user, uid)
            .await
            .map_err(failed)?
        else {
            return Ok(None);
        };
        let row = self
            .mailbox
            .get_message(meta.mailbox_id, uid)
            .await
            .map_err(failed)?;
        self.one(row).await
    }

    async fn message_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> StoreResult<Option<MessageWire>> {
        let row = self
            .mailbox
            .find_message_by_message_id(user, message_id)
            .await
            .map_err(failed)?;
        self.one(row).await
    }

    async fn invite(&self, user: &str, uid: u32) -> StoreResult<Option<String>> {
        let payload: Option<(String,)> = sqlx::query_as(
            "SELECT CAST(m.invite_payload AS TEXT) FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
              WHERE mb.user_address = $1 AND m.uid = $2 AND m.invite_payload IS NOT NULL
              LIMIT 1",
        )
        .bind(user)
        .bind(uid as i32)
        .fetch_optional(&self.pool)
        .await
        .map_err(failed)?;
        Ok(payload.map(|(p,)| p))
    }
}

//! The kevy store's side of reading a user's messages, for the shared
//! routes. Every answer is the user's own copy: the shared blob is stripped
//! of the per-reader fields (file, uid, flags) on every write.

use mailrs_core_api::method::message::MessageWire;
use mailrs_core_routes::store::{MessageReads, StoreError, StoreResult};

use crate::FastcoreState;

fn decoded(bytes: &[u8]) -> StoreResult<MessageWire> {
    serde_json::from_slice(bytes).map_err(|e| StoreError(format!("undecodable message row: {e}")))
}

impl MessageReads for FastcoreState {
    async fn thread_messages(&self, user: &str, thread_id: &str) -> StoreResult<Vec<MessageWire>> {
        // one damaged row leaves the rest of the conversation readable
        Ok(self
            .mailbox
            .list_thread_messages(user, thread_id)?
            .iter()
            .filter_map(|b| {
                decoded(b)
                    .inspect_err(
                        |e| tracing::warn!(error = %e, %user, %thread_id, "message row skipped"),
                    )
                    .ok()
            })
            .collect())
    }

    async fn message_by_uid(&self, user: &str, uid: u32) -> StoreResult<Option<MessageWire>> {
        self.mailbox
            .get_message_by_uid(user, uid)?
            .map(|b| decoded(&b))
            .transpose()
    }

    async fn message_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> StoreResult<Option<MessageWire>> {
        self.mailbox
            .user_message_view(user, message_id)?
            .map(|b| decoded(&b))
            .transpose()
    }

    async fn invite(&self, user: &str, uid: u32) -> StoreResult<Option<String>> {
        let Some(wire) = self.message_by_uid(user, uid).await? else {
            return Ok(None);
        };
        Ok(crate::invites::payload_json(self, &wire.message_id))
    }
}

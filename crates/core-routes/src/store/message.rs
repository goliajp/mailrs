//! Reading a user's messages.

use std::future::Future;

use mailrs_core_api::method::message::MessageWire;

use super::StoreResult;

/// A user's copies of messages, as the store holds them: each answer is
/// that user's copy (their uid, flags and file), never a copy shared with
/// other readers. `None` and an empty list are things the user does not
/// hold, not errors.
pub trait MessageReads: Send + Sync + 'static {
    /// The messages of `thread_id` `user` holds, oldest first.
    fn thread_messages(
        &self,
        user: &str,
        thread_id: &str,
    ) -> impl Future<Output = StoreResult<Vec<MessageWire>>> + Send;

    fn message_by_uid(
        &self,
        user: &str,
        uid: u32,
    ) -> impl Future<Output = StoreResult<Option<MessageWire>>> + Send;

    fn message_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> impl Future<Output = StoreResult<Option<MessageWire>>> + Send;

    /// The typed invitation the message at `uid` carries, as JSON.
    fn invite(
        &self,
        user: &str,
        uid: u32,
    ) -> impl Future<Output = StoreResult<Option<String>>> + Send;
}

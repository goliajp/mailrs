//! A reader's acts on a conversation.

use std::future::Future;

use super::StoreResult;

/// A conversation as one reader sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReaderView {
    /// Whether any of it is unread for them.
    pub unread: bool,
    /// Unix seconds of the latest arrival; 0 when unknown.
    pub latest: i64,
    /// The people in it, comma-separated, as the store lists them.
    pub participants: String,
}

/// The reader's per-conversation state. Each setter does every write the
/// store needs to keep the state, its own durability included, and says
/// whether `user` is in the conversation: a conversation they are not in
/// is not theirs to change, and nothing is written.
pub trait ThreadStore: Send + Sync + 'static {
    /// `None` when `user` is not in it.
    fn reader_view(
        &self,
        user: &str,
        thread_id: &str,
    ) -> impl Future<Output = StoreResult<Option<ReaderView>>> + Send;

    /// Every message of it read for `user`.
    fn mark_read(
        &self,
        user: &str,
        thread_id: &str,
    ) -> impl Future<Output = StoreResult<bool>> + Send;

    /// Unread again for `user`: its newest message.
    fn mark_unread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> impl Future<Output = StoreResult<bool>> + Send;

    fn set_starred(
        &self,
        user: &str,
        thread_id: &str,
        on: bool,
    ) -> impl Future<Output = StoreResult<bool>> + Send;

    fn set_pinned(
        &self,
        user: &str,
        thread_id: &str,
        on: bool,
    ) -> impl Future<Output = StoreResult<bool>> + Send;

    fn set_archived(
        &self,
        user: &str,
        thread_id: &str,
        on: bool,
    ) -> impl Future<Output = StoreResult<bool>> + Send;

    /// Away until `until` (unix seconds); 0 brings it back.
    fn set_snoozed(
        &self,
        user: &str,
        thread_id: &str,
        until: i64,
    ) -> impl Future<Output = StoreResult<()>> + Send;
}

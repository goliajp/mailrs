//! Mail held for review, and letting it go.

use std::future::Future;

use mailrs_core_api::method::thread::ReleaseFact;
use mailrs_core_api::types::ConversationSummaryWire;

use super::StoreResult;

pub trait HoldStore: Send + Sync + 'static {
    /// Up to `n` of `user`'s held conversations last active before
    /// `before_ts` (all when `None`), newest first, as the list shows them,
    /// and how many are held in that range.
    fn held_page(
        &self,
        user: &str,
        before_ts: Option<i64>,
        n: usize,
    ) -> impl Future<Output = StoreResult<(Vec<ConversationSummaryWire>, usize)>> + Send;

    /// Whether `thread_id` is held for `user`; `None` when they are not in
    /// it.
    fn held(
        &self,
        user: &str,
        thread_id: &str,
    ) -> impl Future<Output = StoreResult<Option<bool>>> + Send;

    /// `user` says a conversation held for them is not fraud: it goes back
    /// to its list, and the release is recorded (the fact a later
    /// re-judge consults, and its audit line). Called only for a held one.
    fn release_by_person(
        &self,
        user: &str,
        thread_id: &str,
    ) -> impl Future<Output = StoreResult<()>> + Send;

    /// Queue the remote images of `user`'s conversation, which held mail
    /// did not have fetched on arrival.
    fn queue_images(&self, user: &str, thread_id: &str) -> impl Future<Output = ()> + Send;

    /// Every release of the conversation for `user`, oldest first.
    fn releases(
        &self,
        user: &str,
        thread_id: &str,
    ) -> impl Future<Output = StoreResult<Vec<ReleaseFact>>> + Send;

    /// What was decided about `message_id` when it arrived, joined to where
    /// `user`'s own copy was filed and why; `Some(None)` for a message
    /// nobody suspected, `None` when `user` does not have it (the verdict
    /// is stored by Message-ID for every recipient at once).
    fn fraud_verdict(
        &self,
        user: &str,
        message_id: &str,
    ) -> impl Future<Output = StoreResult<Option<Option<serde_json::Value>>>> + Send;
}

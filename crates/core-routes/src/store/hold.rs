//! Mail held for review, and letting it go.

use std::future::Future;

use mailrs_core_api::method::thread::ReleaseFact;

use super::StoreResult;

pub trait HoldStore: Send + Sync + 'static {
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
}

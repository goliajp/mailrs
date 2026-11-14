//! A user's labels and which conversations carry them.

use std::future::Future;

use mailrs_core_api::method::label::{Label, LabelError};

use super::StoreError;

/// Why a label act did not happen: the request itself was refused, or the
/// store failed.
#[derive(Debug, thiserror::Error)]
pub enum LabelFailure {
    #[error(transparent)]
    Refused(#[from] LabelError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub type LabelResult<T> = Result<T, LabelFailure>;

pub trait LabelStore: Send + Sync + 'static {
    /// `user`'s labels, by name.
    fn labels(&self, user: &str) -> impl Future<Output = LabelResult<Vec<Label>>> + Send;

    fn create_label(
        &self,
        user: &str,
        name: &str,
        color: &str,
        now: i64,
    ) -> impl Future<Output = LabelResult<Label>> + Send;

    fn update_label(
        &self,
        user: &str,
        id: &str,
        name: Option<&str>,
        color: Option<&str>,
    ) -> impl Future<Output = LabelResult<Label>> + Send;

    /// `false` when `user` has no label `id`.
    fn delete_label(&self, user: &str, id: &str) -> impl Future<Output = LabelResult<bool>> + Send;

    /// Put label `id` on `thread_id` or take it off. A label or a
    /// conversation that is not `user`'s is [`LabelError::NotFound`].
    fn set_label(
        &self,
        user: &str,
        id: &str,
        thread_id: &str,
        on: bool,
    ) -> impl Future<Output = LabelResult<()>> + Send;
}

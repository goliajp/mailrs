//! The kevy store's side of labels, for the shared routes.

use mailrs_core_api::method::label::Label;
use mailrs_core_routes::store::{LabelFailure, LabelResult, LabelStore, StoreError};
use mailrs_mailbox_kevy::LabelOpError;

use crate::FastcoreState;

fn failed(e: LabelOpError) -> LabelFailure {
    match e {
        LabelOpError::Label(e) => LabelFailure::Refused(e),
        LabelOpError::Io(e) => LabelFailure::Store(StoreError::from(e)),
    }
}

impl LabelStore for FastcoreState {
    async fn labels(&self, user: &str) -> LabelResult<Vec<Label>> {
        Ok(self.mailbox.labels(user).map_err(StoreError::from)?)
    }

    async fn create_label(
        &self,
        user: &str,
        name: &str,
        color: &str,
        now: i64,
    ) -> LabelResult<Label> {
        self.mailbox
            .create_label(user, name, color, now)
            .map_err(failed)
    }

    async fn update_label(
        &self,
        user: &str,
        id: &str,
        name: Option<&str>,
        color: Option<&str>,
    ) -> LabelResult<Label> {
        self.mailbox
            .update_label(user, id, name, color)
            .map_err(failed)
    }

    async fn delete_label(&self, user: &str, id: &str) -> LabelResult<bool> {
        Ok(self
            .mailbox
            .delete_label(user, id)
            .map_err(StoreError::from)?)
    }

    async fn set_label(&self, user: &str, id: &str, thread_id: &str, on: bool) -> LabelResult<()> {
        self.mailbox
            .set_label(user, id, thread_id, on)
            .map_err(failed)
    }
}

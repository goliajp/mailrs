//! The SQL store's side of labels, for the shared routes.

use mailrs_core_api::method::label::{Label, LabelError};
use mailrs_core_routes::store::{LabelFailure, LabelResult, LabelStore, StoreError};
use mailrs_mailbox::pg::LabelDbError;

use super::CoreRpcState;

fn failed(e: LabelDbError) -> LabelFailure {
    match e {
        LabelDbError::Label(e) => LabelFailure::Refused(e),
        LabelDbError::Db(e) => LabelFailure::Store(StoreError(e.to_string())),
    }
}

impl LabelStore for CoreRpcState {
    async fn labels(&self, user: &str) -> LabelResult<Vec<Label>> {
        self.mailbox
            .labels(user)
            .await
            .map_err(|e| failed(e.into()))
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
            .await
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
            .await
            .map_err(failed)
    }

    async fn delete_label(&self, user: &str, id: &str) -> LabelResult<bool> {
        self.mailbox.delete_label(user, id).await.map_err(failed)
    }

    async fn set_label(&self, user: &str, id: &str, thread_id: &str, on: bool) -> LabelResult<()> {
        // the SQL store would label a conversation the user is not in
        if !self
            .mailbox
            .user_in_thread(user, thread_id)
            .await
            .map_err(|e| failed(e.into()))?
        {
            return Err(LabelError::NotFound.into());
        }
        self.mailbox
            .set_label(user, id, thread_id, on)
            .await
            .map_err(failed)
    }
}

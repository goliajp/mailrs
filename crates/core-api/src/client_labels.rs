//! A user's labels, over the core API.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method::label::{CreateLabelRequest, Label, LabelsResponse, UpdateLabelRequest};

impl Client {
    /// GET /v1/users/{user}/labels — by name.
    pub async fn list_labels(&self, user: &str) -> ApiResult<Vec<Label>> {
        let path = format!("/v1/users/{}/labels", Self::enc(user));
        let resp: LabelsResponse = self.get_authed(path, "list_labels").await?;
        Ok(resp.items)
    }

    /// POST /v1/users/{user}/labels — `Conflict` when the name is taken.
    pub async fn create_label(&self, user: &str, req: &CreateLabelRequest) -> ApiResult<Label> {
        let path = format!("/v1/users/{}/labels", Self::enc(user));
        self.post_authed_json(path, req, "create_label").await
    }

    /// POST /v1/users/{user}/labels/{id} — `Conflict` when the new name is
    /// taken, `NotFound` when there is no such label.
    pub async fn update_label(
        &self,
        user: &str,
        id: &str,
        req: &UpdateLabelRequest,
    ) -> ApiResult<Label> {
        let path = format!("/v1/users/{}/labels/{}", Self::enc(user), Self::enc(id));
        self.post_authed_json(path, req, "update_label").await
    }

    /// DELETE /v1/users/{user}/labels/{id} — the mail stays.
    pub async fn delete_label(&self, user: &str, id: &str) -> ApiResult<()> {
        let path = format!("/v1/users/{}/labels/{}", Self::enc(user), Self::enc(id));
        self.delete_authed(path, "delete_label").await
    }

    /// POST (on) or DELETE (off) /v1/users/{user}/threads/{t}/labels/{id}.
    pub async fn set_thread_label(
        &self,
        user: &str,
        thread_id: &str,
        id: &str,
        on: bool,
    ) -> ApiResult<()> {
        let path = format!(
            "/v1/users/{}/threads/{}/labels/{}",
            Self::enc(user),
            Self::enc(thread_id),
            Self::enc(id)
        );
        match on {
            true => {
                let _: Option<serde_json::Value> =
                    self.post_authed_no_body(path, "label_thread").await?;
                Ok(())
            }
            false => self.delete_authed(path, "unlabel_thread").await,
        }
    }
}

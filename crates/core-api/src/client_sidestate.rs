//! The shared side-state families: drafts, signatures, templates,
//! webhooks, contacts, audit.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method;

impl Client {
    /// GET /v1/users/{user}/drafts
    pub async fn list_drafts(&self, user: &str) -> ApiResult<method::admin::DraftListResponse> {
        let path = format!("/v1/users/{}/drafts", Self::enc(user));
        self.get_authed(path, "list_drafts").await
    }

    /// POST /v1/users/{user}/drafts
    pub async fn save_draft(
        &self,
        user: &str,
        req: &method::admin::SaveDraftRequest,
    ) -> ApiResult<method::admin::SaveDraftResponse> {
        let path = format!("/v1/users/{}/drafts", Self::enc(user));
        self.post_authed_json(path, req, "save_draft").await
    }

    /// DELETE /v1/users/{user}/drafts/{id}
    pub async fn delete_draft(&self, user: &str, id: i64) -> ApiResult<()> {
        let path = format!("/v1/users/{}/drafts/{id}", Self::enc(user));
        self.delete_authed(path, "delete_draft").await
    }

    /// POST /v1/users/{user}/signatures
    pub async fn save_signature(
        &self,
        user: &str,
        req: &method::admin::SaveSignatureRequest,
    ) -> ApiResult<method::admin::SaveSignatureResponse> {
        let path = format!("/v1/users/{}/signatures", Self::enc(user));
        self.post_authed_json(path, req, "save_signature").await
    }

    /// DELETE /v1/users/{user}/signatures/{id}
    pub async fn delete_signature(&self, user: &str, id: i64) -> ApiResult<()> {
        let path = format!("/v1/users/{}/signatures/{id}", Self::enc(user));
        self.delete_authed(path, "delete_signature").await
    }

    /// POST /v1/admin/webhook-subscriptions
    pub async fn create_webhook(
        &self,
        req: &method::admin::CreateWebhookRequest,
    ) -> ApiResult<method::admin::CreateWebhookResponse> {
        self.post_authed_json(
            "/v1/admin/webhook-subscriptions".to_string(),
            req,
            "create_webhook",
        )
        .await
    }

    /// GET /v1/admin/accounts/{address}/webhook-subscriptions
    pub async fn list_webhooks(
        &self,
        address: &str,
    ) -> ApiResult<method::admin::WebhookListResponse> {
        let path = format!(
            "/v1/admin/accounts/{}/webhook-subscriptions",
            Self::enc(address)
        );
        self.get_authed(path, "list_webhooks").await
    }

    /// DELETE /v1/admin/webhook-subscriptions/{id}
    pub async fn delete_webhook(&self, id: i64) -> ApiResult<()> {
        let path = format!("/v1/admin/webhook-subscriptions/{id}");
        self.delete_authed(path, "delete_webhook").await
    }

    /// GET /v1/users/{user}/templates
    pub async fn list_templates(
        &self,
        user: &str,
    ) -> ApiResult<method::admin::TemplateListResponse> {
        let path = format!("/v1/users/{}/templates", Self::enc(user));
        self.get_authed(path, "list_templates").await
    }

    /// POST /v1/users/{user}/templates
    pub async fn save_template(
        &self,
        user: &str,
        req: &method::admin::SaveTemplateRequest,
    ) -> ApiResult<method::admin::SaveTemplateResponse> {
        let path = format!("/v1/users/{}/templates", Self::enc(user));
        self.post_authed_json(path, req, "save_template").await
    }

    /// DELETE /v1/users/{user}/templates/{id}
    pub async fn delete_template(&self, user: &str, id: i64) -> ApiResult<()> {
        let path = format!("/v1/users/{}/templates/{id}", Self::enc(user));
        self.delete_authed(path, "delete_template").await
    }
}

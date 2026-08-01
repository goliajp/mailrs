//! The shared side-state families: drafts, signatures, templates,
//! webhooks, reactions, contacts, audit.

use crate::client::Client;
use crate::error::{ApiResult, CoreApiError};
use crate::method;

impl Client {
    /// GET /v1/admin/audit-log
    pub async fn list_audit_log(&self, limit: u32) -> ApiResult<method::admin::AuditListResponse> {
        let path = format!("/v1/admin/audit-log?limit={limit}");
        self.get_authed(path, "list_audit_log").await
    }

    /// POST /v1/admin/audit-log — fire-and-forget audit trail write.
    ///
    /// v2.7.2 §Phase 12 §12.1: called from admin write handlers'
    /// success branches. `detail` is free-form text (JSON string OK
    /// for structured actions). Non-blocking best-effort — a network
    /// hiccup here must not break the business write, per RFC
    /// `20260610-audit-log-retrofit.md` failure-mode decision.
    pub async fn log_audit(
        &self,
        actor: &str,
        action: &str,
        target: &str,
        detail: &str,
    ) -> ApiResult<()> {
        let req = method::admin::LogAuditRequest {
            actor: actor.into(),
            action: action.into(),
            target: target.into(),
            detail: detail.into(),
        };
        self.post_authed_no_content("/v1/admin/audit-log", "log_audit", &req)
            .await
    }

    /// POST /v1/users/{user}/contacts/{email}/feedback
    pub async fn sender_feedback(&self, user: &str, email: &str, action: &str) -> ApiResult<()> {
        let path = format!(
            "/v1/users/{}/contacts/{}/feedback",
            Self::enc(user),
            Self::enc(email),
        );
        let req = method::contact::SenderFeedbackRequest {
            action: action.to_string(),
            bias_delta: None,
        };
        let resp = self
            .inner
            .post(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .json(&req)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("sender_feedback transport: {e}")))?;
        Self::map_status_unit(resp, "sender_feedback").await
    }

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

    /// GET /v1/users/{user}/signatures
    pub async fn list_signatures(
        &self,
        user: &str,
    ) -> ApiResult<method::admin::SignatureListResponse> {
        let path = format!("/v1/users/{}/signatures", Self::enc(user));
        self.get_authed(path, "list_signatures").await
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

    /// GET /v1/users/{user}/threads/{thread_id}/reactions
    pub async fn get_thread_reactions(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::admin::ReactionsResponse> {
        let path = format!(
            "/v1/users/{}/threads/{}/reactions",
            Self::enc(user),
            Self::enc(thread_id),
        );
        self.get_authed(path, "get_thread_reactions").await
    }

    /// PUT /v1/users/{user}/threads/{thread_id}/messages/{uid}/reactions
    pub async fn toggle_reaction(
        &self,
        user: &str,
        thread_id: &str,
        uid: i64,
        req: &method::admin::ToggleReactionRequest,
    ) -> ApiResult<method::admin::ReactionsResponse> {
        let path = format!(
            "/v1/users/{}/threads/{}/messages/{uid}/reactions",
            Self::enc(user),
            Self::enc(thread_id),
        );
        self.put_authed_json_returning(path, req, "toggle_reaction")
            .await
    }

    /// GET /v1/users/{user}/contacts:search?q=&limit=
    pub async fn search_contacts(
        &self,
        user: &str,
        q: &str,
        limit: u32,
    ) -> ApiResult<method::contact::SearchContactsResponse> {
        let path = format!(
            "/v1/users/{}/contacts:search?q={}&limit={limit}",
            Self::enc(user),
            Self::enc(q),
        );
        self.get_authed(path, "search_contacts").await
    }
}

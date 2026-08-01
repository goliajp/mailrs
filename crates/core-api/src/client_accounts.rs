//! Account, alias, domain and permission RPCs.

use crate::client::Client;
use crate::error::{ApiResult, CoreApiError};
use crate::method;

impl Client {
    /// GET /v1/admin/accounts/{address}/effective-permissions
    pub async fn effective_permissions(
        &self,
        address: &str,
    ) -> ApiResult<method::admin::EffectivePermissionsResponse> {
        let path = format!(
            "/v1/admin/accounts/{}/effective-permissions",
            Self::enc(address)
        );
        self.get_authed(path, "effective_permissions").await
    }

    /// GET /v1/admin/api-keys/by-prefix/{prefix}
    pub async fn api_key_by_prefix(&self, prefix: &str) -> ApiResult<method::admin::ApiKeyWire> {
        let path = format!("/v1/admin/api-keys/by-prefix/{}", Self::enc(prefix));
        self.get_authed(path, "api_key_by_prefix").await
    }

    /// POST /v1/admin/api-keys/{id}/touch
    pub async fn touch_api_key(&self, id: i64) -> ApiResult<()> {
        let path = format!("/v1/admin/api-keys/{id}/touch");
        let resp = self
            .inner
            .post(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("touch_api_key transport: {e}")))?;
        Self::map_status_unit(resp, "touch_api_key").await
    }

    /// POST /v1/admin/accounts — create account
    pub async fn add_account(&self, req: &method::admin::AddAccountRequest) -> ApiResult<()> {
        let resp = self
            .inner
            .post(self.url("/v1/admin/accounts"))
            .bearer_auth(&self.auth_bearer)
            .json(req)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("add_account transport: {e}")))?;
        Self::map_status_unit(resp, "add_account").await
    }

    /// DELETE /v1/admin/accounts/{address}
    pub async fn remove_account(&self, address: &str) -> ApiResult<()> {
        let path = format!("/v1/admin/accounts/{}", Self::enc(address));
        self.delete_authed(path, "remove_account").await
    }

    /// PUT /v1/admin/accounts/{address} — patch display_name.
    pub async fn update_account(
        &self,
        address: &str,
        req: &method::admin::UpdateAccountRequest,
    ) -> ApiResult<()> {
        let path = format!("/v1/admin/accounts/{}", Self::enc(address));
        self.put_authed_json(path, req, "update_account").await
    }

    /// POST /v1/admin/accounts/{address}/quota
    pub async fn set_quota(
        &self,
        address: &str,
        req: &method::admin::SetQuotaRequest,
    ) -> ApiResult<()> {
        let path = format!("/v1/admin/accounts/{}/quota", Self::enc(address));
        self.post_authed_json(path, req, "set_quota").await
    }

    /// POST /v1/admin/accounts/{address}/recovery-email
    pub async fn set_recovery_email(
        &self,
        address: &str,
        req: &method::admin::UpdateRecoveryEmailRequest,
    ) -> ApiResult<()> {
        let path = format!("/v1/admin/accounts/{}/recovery-email", Self::enc(address));
        self.post_authed_json(path, req, "set_recovery_email").await
    }

    /// POST /v1/admin/accounts/{address}/password — persist a
    /// pre-hashed password. Webapi hashes locally so fastcore never
    /// sees plaintext.
    pub async fn set_account_password(
        &self,
        address: &str,
        req: &method::admin::SetPasswordRequest,
    ) -> ApiResult<()> {
        // cores answer 204 No Content — decode a unit, don't parse a body
        // (post_authed_json's map_status::<()> would fail on the empty body;
        // caught by the core-sync round-trip test).
        let resp = self
            .inner
            .post(self.url(&format!(
                "/v1/admin/accounts/{}/password",
                Self::enc(address)
            )))
            .bearer_auth(&self.auth_bearer)
            .json(req)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("set_account_password transport: {e}")))?;
        Self::map_status_unit(resp, "set_account_password").await
    }

    /// POST /v1/admin/aliases
    pub async fn add_alias(
        &self,
        req: &method::admin::AddAliasRequest,
    ) -> ApiResult<method::admin::AddAliasResponse> {
        self.post_authed_json("/v1/admin/aliases".to_string(), req, "add_alias")
            .await
    }

    /// DELETE /v1/admin/aliases/{id}
    pub async fn remove_alias(&self, id: i64) -> ApiResult<()> {
        let path = format!("/v1/admin/aliases/{id}");
        self.delete_authed(path, "remove_alias").await
    }

    /// POST /v1/admin/domains
    pub async fn add_domain(&self, name: &str) -> ApiResult<()> {
        let req = method::admin::AddDomainRequest {
            name: name.to_string(),
        };
        let resp = self
            .inner
            .post(self.url("/v1/admin/domains"))
            .bearer_auth(&self.auth_bearer)
            .json(&req)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("add_domain transport: {e}")))?;
        Self::map_status_unit(resp, "add_domain").await
    }

    /// DELETE /v1/admin/domains/{name}
    pub async fn remove_domain(&self, name: &str) -> ApiResult<()> {
        let path = format!("/v1/admin/domains/{}", Self::enc(name));
        self.delete_authed(path, "remove_domain").await
    }

    /// GET /v1/admin/accounts — list all
    pub async fn list_accounts(&self) -> ApiResult<method::admin::AccountListResponse> {
        self.get_authed("/v1/admin/accounts".to_string(), "list_accounts")
            .await
    }

    /// GET /v1/admin/aliases — list all
    pub async fn list_aliases(&self) -> ApiResult<method::admin::AliasListResponse> {
        self.get_authed("/v1/admin/aliases".to_string(), "list_aliases")
            .await
    }

    /// GET /v1/admin/domains — list all
    pub async fn list_domains(&self) -> ApiResult<method::admin::DomainListResponse> {
        self.get_authed("/v1/admin/domains".to_string(), "list_domains")
            .await
    }

    /// GET /v1/admin/accounts/{address}/credentials
    pub async fn get_account_with_hash(
        &self,
        address: &str,
    ) -> ApiResult<method::admin::AccountWithHashWire> {
        let path = format!("/v1/admin/accounts/{}/credentials", Self::enc(address));
        self.get_authed(path, "get_account_with_hash").await
    }

    /// GET /v1/admin/aliases:local — every alias currently in fastcore.
    pub async fn list_local_aliases(&self) -> ApiResult<serde_json::Value> {
        self.get_authed("/v1/admin/aliases:local".to_string(), "list_local_aliases")
            .await
    }

    /// POST /v1/admin/aliases:local — insert/replace one alias.
    pub async fn upsert_local_alias(&self, source: &str, target: &str) -> ApiResult<()> {
        let body = serde_json::json!({"source": source, "target": target});
        let resp = self
            .inner
            .post(self.url("/v1/admin/aliases:local"))
            .bearer_auth(&self.auth_bearer)
            .json(&body)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("upsert_local_alias transport: {e}")))?;
        Self::map_status_unit(resp, "upsert_local_alias").await
    }

    /// DELETE /v1/admin/aliases:local/{source}
    pub async fn delete_local_alias(&self, source: &str) -> ApiResult<()> {
        let path = format!("/v1/admin/aliases:local/{}", Self::enc(source));
        self.delete_authed(path, "delete_local_alias").await
    }
}

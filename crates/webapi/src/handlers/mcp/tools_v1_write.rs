//! The second half of the v1 MCP tools, in their own named router.
//!
//! Split out of `mcp/mod.rs` on 2026-08-02, which held all thirty-seven
//! v1 tools plus the service type, the router mount and the auth
//! middleware — 1,033 lines. Same shape as the `tools_v2_batch*` files:
//! one `#[tool_router]` block, combined in `mod.rs`.
//!
//! Tool **names** are the wire contract and must stay identical to the
//! monolith lane; `scripts/check-mcp-parity.sh` fails when they diverge
//! (`.claude/rules/mcp-two-lane-parity.md`). Moving a tool between files
//! does not change its name, which is why this is safe to do at all.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use super::ok_result;
use super::params::*;

#[tool_router(router = tool_router_v1_write, vis = "pub")]
impl MailrsMcpService {
    #[tool(description = "List all managed domains (requires an admin permission).")]
    async fn list_domains(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        // domains are shared side-state (env + network kevy admin:domains),
        // not switchable-core data — read network kevy directly.
        let flat = crate::handlers::kevy_util::with_kevy(|c| {
            c.hgetall(b"admin:domains").map_err(std::io::Error::other)
        })
        .map_err(|_| McpError::internal_error("domains read failed", None))?;
        let items: Vec<serde_json::Value> = flat
            .chunks(2)
            .filter_map(|p| p.get(1))
            .filter_map(|v| serde_json::from_slice(v).ok())
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "domains": items }).to_string(),
        )]))
    }

    #[tool(description = "Create a new account (requires an admin permission).")]
    async fn create_account(
        &self,
        Parameters(params): Parameters<CreateAccountParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let req = mailrs_core_api::method::admin::AddAccountRequest {
            address: params.address.clone(),
            display_name: params.display_name,
            password: params.password,
        };
        self.state
            .core
            .add_account(&req)
            .await
            .map_err(|e| McpError::internal_error(format!("create_account: {e}"), None))?;
        crate::handlers::audit::record(&user, "account.create", &params.address, "via mcp");
        Ok(ok_result())
    }

    #[tool(description = "Remove an account (requires an admin permission).")]
    async fn remove_account(
        &self,
        Parameters(params): Parameters<AddressParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .remove_account(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("remove_account: {e}"), None))?;
        crate::handlers::audit::record(&user, "account.delete", &params.address, "via mcp");
        Ok(ok_result())
    }

    #[tool(
        description = "Add an email alias/forward (requires an admin permission). Type 'alias' delivers to a local account; 'forward' relays outbound."
    )]
    async fn add_alias(
        &self,
        Parameters(params): Parameters<AddAliasParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let domain = params
            .source_address
            .rsplit_once('@')
            .map(|(_, d)| d.to_string())
            .unwrap_or_default();
        let req = mailrs_core_api::method::admin::AddAliasRequest {
            source_address: params.source_address.clone(),
            target_address: params.target_address.clone(),
            domain,
            alias_type: params.alias_type.unwrap_or_else(|| "alias".into()),
        };
        let resp = self
            .state
            .core
            .add_alias(&req)
            .await
            .map_err(|e| McpError::internal_error(format!("add_alias: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "alias.create",
            &params.source_address,
            &format!("→ {}", params.target_address),
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Remove an alias by id (requires an admin permission).")]
    async fn remove_alias(
        &self,
        Parameters(params): Parameters<RemoveAliasParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .remove_alias(params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("remove_alias: {e}"), None))?;
        crate::handlers::audit::record(&user, "alias.delete", &params.id.to_string(), "via mcp");
        Ok(ok_result())
    }

    #[tool(description = "Add a managed domain (requires an admin permission).")]
    async fn add_domain(
        &self,
        Parameters(params): Parameters<DomainNameParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .add_domain(&params.name)
            .await
            .map_err(|e| McpError::internal_error(format!("add_domain: {e}"), None))?;
        crate::handlers::audit::record(&user, "domain.create", &params.name, "via mcp");
        Ok(ok_result())
    }

    #[tool(
        description = "Remove a managed domain and its dependent accounts/aliases (requires an admin permission)."
    )]
    async fn remove_domain(
        &self,
        Parameters(params): Parameters<DomainNameParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .remove_domain(&params.name)
            .await
            .map_err(|e| McpError::internal_error(format!("remove_domain: {e}"), None))?;
        crate::handlers::audit::record(&user, "domain.delete", &params.name, "via mcp");
        Ok(ok_result())
    }

    #[tool(
        description = "Save or update the caller's own email signature. Returns the new id — pass it to `delete_signature` to remove."
    )]
    async fn save_signature(
        &self,
        Parameters(params): Parameters<SaveSignatureParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = mailrs_core_api::method::admin::SaveSignatureRequest {
            name: params.name.clone(),
            html: params.html,
            text_content: params.text_content,
            is_default: params.is_default,
        };
        let resp = self
            .state
            .core
            .save_signature(&user, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("save_signature: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "signature.save",
            &format!("id={}", resp.id),
            &params.name,
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Delete one of the caller's own signatures by id.")]
    async fn delete_signature(
        &self,
        Parameters(params): Parameters<SignatureIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .delete_signature(&user, params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_signature: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "signature.delete",
            &format!("id={}", params.id),
            "via mcp",
        );
        Ok(ok_result())
    }

    #[tool(
        description = "List webhook subscriptions for an account (requires admin OR the caller owning the account)."
    )]
    async fn list_webhooks(
        &self,
        Parameters(params): Parameters<AddressParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        if user != params.address {
            self.require_admin(&user).await?;
        }
        let resp = self
            .state
            .core
            .list_webhooks(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("list_webhooks: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(
        description = "Create a webhook subscription. Returns id + signing_secret — store the secret, it isn't returned again."
    )]
    async fn create_webhook(
        &self,
        Parameters(params): Parameters<CreateWebhookParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        if user != params.account_address {
            self.require_admin(&user).await?;
        }
        let req = mailrs_core_api::method::admin::CreateWebhookRequest {
            account_address: params.account_address.clone(),
            url: params.url.clone(),
            event_type: params.event_type.clone(),
            filter_sender: params.filter_sender,
            filter_thread_id: params.filter_thread_id,
        };
        let resp = self
            .state
            .core
            .create_webhook(&req)
            .await
            .map_err(|e| McpError::internal_error(format!("create_webhook: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "webhook.create",
            &params.account_address,
            &format!("url={} event={}", params.url, params.event_type),
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Delete a webhook subscription by id (requires an admin permission).")]
    async fn delete_webhook(
        &self,
        Parameters(params): Parameters<WebhookIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .delete_webhook(params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_webhook: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "webhook.delete",
            &format!("id={}", params.id),
            "via mcp",
        );
        Ok(ok_result())
    }

    #[tool(description = "List the caller's own saved drafts.")]
    async fn list_drafts(
        &self,
        Parameters(params): Parameters<ListDraftsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let mut resp = self
            .state
            .core
            .list_drafts(&user)
            .await
            .map_err(|e| McpError::internal_error(format!("list_drafts: {e}"), None))?;
        if let Some(limit) = params.limit {
            resp.items.truncate(limit as usize);
        }
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(
        description = "Save a draft for the caller. Idempotent per body — returns the new id; overwrite by re-saving."
    )]
    async fn save_draft(
        &self,
        Parameters(params): Parameters<SaveDraftParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = mailrs_core_api::method::admin::SaveDraftRequest {
            id: None,
            to: params.to,
            cc: params.cc,
            bcc: params.bcc,
            subject: params.subject.clone(),
            body: params.body,
            reply_to_thread_id: params.reply_to_thread_id,
        };
        let resp = self
            .state
            .core
            .save_draft(&user, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("save_draft: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "draft.save",
            &format!("id={}", resp.id),
            &params.subject,
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Delete one of the caller's own drafts by id.")]
    async fn delete_draft(
        &self,
        Parameters(params): Parameters<DraftIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .delete_draft(&user, params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_draft: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "draft.delete",
            &format!("id={}", params.id),
            "via mcp",
        );
        Ok(ok_result())
    }

    #[tool(description = "List the caller's own compose templates.")]
    async fn list_templates(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let resp = self
            .state
            .core
            .list_templates(&user)
            .await
            .map_err(|e| McpError::internal_error(format!("list_templates: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(
        description = "Save or update the caller's own compose template. Returns the new id — pass it to `delete_template` to remove."
    )]
    async fn save_template(
        &self,
        Parameters(params): Parameters<SaveTemplateParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = mailrs_core_api::method::admin::SaveTemplateRequest {
            name: params.name.clone(),
            subject: params.subject,
            html_body: params.html_body,
            text_body: params.text_body,
            category: params.category,
            is_default: params.is_default,
        };
        let resp = self
            .state
            .core
            .save_template(&user, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("save_template: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "template.save",
            &format!("id={}", resp.id),
            &params.name,
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Delete one of the caller's own compose templates by id.")]
    async fn delete_template(
        &self,
        Parameters(params): Parameters<TemplateIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .delete_template(&user, params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_template: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "template.delete",
            &format!("id={}", params.id),
            "via mcp",
        );
        Ok(ok_result())
    }

    #[tool(description = "Read the admin audit log, newest first (requires an admin permission).")]
    async fn get_audit_log(
        &self,
        Parameters(params): Parameters<AuditQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let limit = params.limit as i64;
        let rows = crate::handlers::kevy_util::with_kevy(move |c| {
            c.lrange(b"admin:audit_log", 0, limit - 1)
                .map_err(std::io::Error::other)
        })
        .map_err(|_| McpError::internal_error("audit read failed", None))?;
        let items: Vec<serde_json::Value> = rows
            .into_iter()
            .filter_map(|v| serde_json::from_slice(&v).ok())
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "entries": items }).to_string(),
        )]))
    }
}

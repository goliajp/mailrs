//! v1 directory tools that change something.
//!
//! Second half of the `tools_v1_directory` split (2026-08-02).
use super::MailMcpService;
use super::tools::*;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

#[tool_router(router = tool_router_v1_directory_write, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(
        description = "Create a new email account (onboarding). Requires admin.accounts permission. Automatically adds account to the domain's default user group."
    )]
    async fn create_account(
        &self,
        Parameters(params): Parameters<CreateAccountParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        if let Err(e) = crate::users::validate_email(&params.address) {
            return Err(McpError::invalid_params(e, None));
        }
        let ds = self
            .web_state
            .domain_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let password_hash = if params.password.is_empty() {
            String::new()
        } else {
            if let Err(e) = crate::users::validate_password(&params.password) {
                return Err(McpError::invalid_params(e, None));
            }
            crate::users::UserStore::hash_password(&params.password)
                .map_err(|_| McpError::internal_error("failed to hash password", None))?
        };

        ds.add_account(
            &params.address,
            &params.domain,
            &params.display_name,
            &password_hash,
            0,
        )
        .await
        .map_err(|e| McpError::internal_error(format!("failed to create account: {e}"), None))?;

        ds.log_audit(
            &self.auth_user.address,
            "account_created",
            &params.address,
            &format!("domain={}", params.domain),
        )
        .await;

        // auto-add to domain's user group
        let groups = ds
            .list_groups(Some(&params.domain))
            .await
            .unwrap_or_default();
        if let Some(user_group) = groups
            .iter()
            .find(|g| g.name == "user" && g.domain.as_deref() == Some(&params.domain))
        {
            let _ = ds
                .add_account_to_group(&params.address, user_group.id)
                .await;
        }

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "created", "address": params.address}).to_string(),
        )]))
    }

    #[tool(
        description = "Remove an email account (offboarding). Requires admin.accounts permission. Removes account, group memberships, and permission overrides."
    )]
    async fn remove_account(
        &self,
        Parameters(params): Parameters<RemoveAccountParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self
            .web_state
            .domain_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let removed = ds.remove_account(&params.address).await.map_err(|e| {
            McpError::internal_error(format!("failed to remove account: {e}"), None)
        })?;

        if removed {
            ds.log_audit(
                &self.auth_user.address,
                "account_removed",
                &params.address,
                "",
            )
            .await;
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({"status": "removed", "address": params.address}).to_string(),
            )]))
        } else {
            Err(McpError::invalid_params("account not found", None))
        }
    }

    #[tool(description = "Reset an account's password. Requires admin.accounts permission.")]
    async fn set_account_password(
        &self,
        Parameters(params): Parameters<SetAccountPasswordParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self
            .web_state
            .domain_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let password_hash = crate::users::UserStore::hash_password(&params.password)
            .map_err(|e| McpError::internal_error(format!("failed to hash password: {e}"), None))?;

        // re-add account with new password (upsert)
        let existing = ds
            .get_account_with_hash(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?
            .ok_or_else(|| McpError::invalid_params("account not found", None))?;

        ds.add_account(
            &params.address,
            &existing.0.domain,
            &existing.0.display_name,
            &password_hash,
            0,
        )
        .await
        .map_err(|e| McpError::internal_error(format!("failed to update password: {e}"), None))?;

        ds.log_audit(
            &self.auth_user.address,
            "password_reset",
            &params.address,
            "",
        )
        .await;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "password_updated", "address": params.address})
                .to_string(),
        )]))
    }

    #[tool(
        description = "Add an account to a permission group. Use list_groups to find group IDs. Requires admin.groups permission."
    )]
    async fn add_account_to_group(
        &self,
        Parameters(params): Parameters<AddAccountToGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.groups")?;
        let ds = self
            .web_state
            .domain_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        ds.add_account_to_group(&params.address, params.group_id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        ds.log_audit(
            &self.auth_user.address,
            "group_member_added",
            &params.group_id.to_string(),
            &format!("address={}", params.address),
        )
        .await;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "added", "address": params.address, "group_id": params.group_id}).to_string(),
        )]))
    }

    #[tool(
        description = "Remove an account from a permission group. Requires admin.groups permission."
    )]
    async fn remove_account_from_group(
        &self,
        Parameters(params): Parameters<RemoveAccountFromGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.groups")?;
        let ds = self
            .web_state
            .domain_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let removed = ds
            .remove_account_from_group(&params.address, params.group_id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        if removed {
            ds.log_audit(
                &self.auth_user.address,
                "group_member_removed",
                &params.group_id.to_string(),
                &format!("address={}", params.address),
            )
            .await;
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({"status": "removed", "address": params.address, "group_id": params.group_id}).to_string(),
            )]))
        } else {
            Err(McpError::invalid_params("membership not found", None))
        }
    }

    #[tool(description = "Add a new domain. Requires admin.domains permission.")]
    async fn add_domain(
        &self,
        Parameters(params): Parameters<AddDomainParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.domains")?;
        let ds = self.ds()?;
        ds.add_domain(&params.name, chrono::Utc::now().timestamp())
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        ds.log_audit(
            &self.auth_user.address,
            "domain_added",
            &params.name,
            &format!("name={}", params.name),
        )
        .await;
        self.ok_result("domain_added", &params.name)
    }

    #[tool(
        description = "Remove a domain and all its accounts/aliases. Requires admin.domains permission."
    )]
    async fn remove_domain(
        &self,
        Parameters(params): Parameters<RemoveDomainParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.domains")?;
        let ds = self.ds()?;
        let removed = ds
            .remove_domain(&params.name)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            ds.log_audit(
                &self.auth_user.address,
                "domain_removed",
                &params.name,
                &format!("name={}", params.name),
            )
            .await;
            self.ok_result("domain_removed", &params.name)
        } else {
            Err(McpError::invalid_params("domain not found", None))
        }
    }

    #[tool(
        description = "Add an email alias or forward. Type 'alias' delivers to local account, 'forward' relays externally. Requires admin.aliases permission."
    )]
    async fn add_alias(
        &self,
        Parameters(params): Parameters<AddAliasParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.aliases")?;
        let ds = self.ds()?;
        let id = ds
            .add_alias(
                &params.source_address,
                &params.target_address,
                &params.domain,
                &params.alias_type,
                0,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        ds.log_audit(
            &self.auth_user.address,
            "alias_added",
            &id.to_string(),
            &format!(
                "source={} target={} type={}",
                params.source_address, params.target_address, params.alias_type
            ),
        )
        .await;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "alias_added", "id": id, "source": params.source_address, "target": params.target_address}).to_string(),
        )]))
    }

    #[tool(description = "Remove an email alias by ID. Requires admin.aliases permission.")]
    async fn remove_alias(
        &self,
        Parameters(params): Parameters<RemoveAliasParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.aliases")?;
        let ds = self.ds()?;
        let removed = ds
            .remove_alias(params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            ds.log_audit(
                &self.auth_user.address,
                "alias_removed",
                &params.id.to_string(),
                "",
            )
            .await;
            self.ok_result("alias_removed", &params.id.to_string())
        } else {
            Err(McpError::invalid_params("alias not found", None))
        }
    }

    #[tool(
        description = "Register a new app and generate its API key. The key is only returned once. Requires admin.accounts permission."
    )]
    async fn create_app(
        &self,
        Parameters(params): Parameters<CreateAppParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let pool = self
            .web_state
            .pg_pool
            .as_ref()
            .ok_or_else(|| McpError::internal_error("database unavailable", None))?;

        let app_id = uuid::Uuid::new_v4().to_string();
        let id = ds
            .create_app(
                &app_id,
                &params.name,
                &params.description,
                &self.auth_user.address,
                &params.scopes,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        let (full_key, prefix, key_hash) = crate::api_key_store::generate_api_key();
        let key_id = crate::api_key_store::insert_app_api_key(
            pool,
            &prefix,
            &key_hash,
            &full_key,
            &self.auth_user.address,
            &params.name,
            id,
            None,
        )
        .await
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        ds.log_audit(
            &self.auth_user.address,
            "app_created",
            &app_id,
            &format!("name={}", params.name),
        )
        .await;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "app_id": app_id, "name": params.name, "scopes": params.scopes,
                "api_key": {"id": key_id, "key": full_key, "prefix": prefix},
                "warning": "Save this API key now. It cannot be retrieved again.",
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Delete an app and revoke all its API keys. Requires admin.accounts permission."
    )]
    async fn delete_app(
        &self,
        Parameters(params): Parameters<DeleteAppParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let removed = ds
            .remove_app(&params.app_id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            ds.log_audit(&self.auth_user.address, "app_deleted", &params.app_id, "")
                .await;
            self.ok_result("app_deleted", &params.app_id)
        } else {
            Err(McpError::invalid_params("app not found", None))
        }
    }
}

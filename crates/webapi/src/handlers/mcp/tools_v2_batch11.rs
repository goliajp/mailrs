//! v2 MCP tool batch 11 — account credentials + permission-group
//! membership. Two-lane parity with the monolith's `set_account_password`
//! / `get_account_permissions` / `add_account_to_group` /
//! `remove_account_from_group`.
//!
//! Data access mirrors the fastcore REST handlers: the password goes
//! through the `set_account_password` core RPC (webapi hashes locally,
//! same as `handlers::auth::change_password`), group membership lives
//! in the network-kevy `admin:groups:<id>:members` sets written by
//! `handlers::complete::{add,remove}_group_member`.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::params::{AccountGroupParams, AddressParams, SetAccountPasswordParams};
use super::{MailrsMcpService, ok_result};
use crate::handlers::kevy_util::with_kevy;

fn group_members_key(group_id: i64) -> String {
    format!("admin:groups:{group_id}:members")
}

#[tool_router(router = tool_router_v2_batch11, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Reset an account's password. The plaintext is Argon2-hashed here and only the hash reaches the mail store. Admin-gated."
    )]
    async fn set_account_password(
        &self,
        Parameters(params): Parameters<SetAccountPasswordParams>,
    ) -> Result<CallToolResult, McpError> {
        use argon2::{
            Argon2,
            password_hash::{PasswordHasher, SaltString, rand_core::OsRng as ArgonRng},
        };
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        if params.password.is_empty() {
            return Err(McpError::invalid_params("password is required", None));
        }
        let salt = SaltString::generate(&mut ArgonRng);
        let hash = Argon2::default()
            .hash_password(params.password.as_bytes(), &salt)
            .map_err(|e| McpError::internal_error(format!("hash: {e}"), None))?
            .to_string();
        let req = mailrs_core_api::method::admin::SetPasswordRequest {
            password_hash: hash,
        };
        self.state
            .core
            .set_account_password(&params.address, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("set_account_password: {e}"), None))?;
        crate::handlers::audit::record(&user, "account.password_reset", &params.address, "via mcp");
        Ok(ok_result())
    }

    #[tool(
        description = "List the permission groups an account belongs to. Scans `list_groups` and reports which ones carry the address. Admin-gated."
    )]
    async fn get_account_permissions(
        &self,
        Parameters(params): Parameters<AddressParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        // Same RPC `require_admin` itself resolves through, so the
        // answer here is exactly what the gate would decide.
        let perms = self
            .state
            .core
            .effective_permissions(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("effective_permissions: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "address": perms.address,
                "permissions": perms.permissions,
                "groups": perms.groups,
                "is_super": perms.is_super,
                "send_as": perms.send_as,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Add an account to a permission group. Use `list_groups` to find group ids. Admin-gated."
    )]
    async fn add_account_to_group(
        &self,
        Parameters(params): Parameters<AccountGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let key = group_members_key(params.group_id);
        let addr = params.address.clone();
        with_kevy(move |c| {
            c.sadd(key.as_bytes(), &[addr.as_bytes()])?;
            Ok(())
        })
        .map_err(|_| McpError::internal_error("group member add failed", None))?;
        crate::handlers::audit::record(
            &user,
            "group.member_add",
            &params.group_id.to_string(),
            &params.address,
        );
        Ok(ok_result())
    }

    #[tool(description = "Remove an account from a permission group. Admin-gated.")]
    async fn remove_account_from_group(
        &self,
        Parameters(params): Parameters<AccountGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let key = group_members_key(params.group_id);
        let addr = params.address.clone();
        with_kevy(move |c| {
            c.srem(key.as_bytes(), &[addr.as_bytes()])?;
            Ok(())
        })
        .map_err(|_| McpError::internal_error("group member remove failed", None))?;
        crate::handlers::audit::record(
            &user,
            "group.member_remove",
            &params.group_id.to_string(),
            &params.address,
        );
        Ok(ok_result())
    }
}

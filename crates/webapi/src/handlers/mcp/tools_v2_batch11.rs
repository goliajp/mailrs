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
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

use super::params::{AccountGroupParams, AddressParams, SetAccountPasswordParams};
use super::{MailrsMcpService, ok_result};
use crate::handlers::kevy_util::with_kevy;

use mailrs_core_sidestate::families::permission_groups::members_key as group_members_key;

#[tool_router(router = tool_router_v2_batch11, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Reset an account's password and sign its sessions out. The plaintext is Argon2-hashed here and only the hash reaches the mail store. Agent keys stay, unless `compromised_revoke_key_ids` is given: then every agent key is revoked, and it must list the ids of all of them as `list_account_agent_keys` shows them (otherwise `key_list_changed` with the current keys). Needs admin.accounts and a recent confirmation."
    )]
    async fn set_account_password(
        &self,
        Parameters(params): Parameters<SetAccountPasswordParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::handlers::admin_directory::ResetRefused;
        let user = self.require_user()?.to_string();
        self.require_confirmed()?;
        crate::handlers::kevy_util::require_permission(&self.state, &user, "admin.accounts")
            .await
            .map_err(|_| McpError::invalid_request("admin.accounts permission required", None))?;
        let named = params.compromised_revoke_key_ids.as_deref();
        let done = crate::handlers::admin_directory::reset_password(
            &self.state,
            &user,
            &params.address,
            &params.password,
            named,
            "via mcp",
        )
        .await;
        match done {
            Ok(done) => Ok(CallToolResult::success(vec![ContentBlock::text(
                serde_json::json!({ "ok": true, "sessions_revoked": done.sessions_revoked, "agent_keys_revoked": done.agent_keys_revoked }).to_string(),
            )])),
            Err(ResetRefused::Weak(_)) => Err(McpError::invalid_params(
                "the new password does not meet the password rules",
                None,
            )),
            Err(ResetRefused::KeyListChanged(keys)) => Err(McpError::invalid_request(
                "key_list_changed",
                Some(serde_json::json!({ "keys": keys })),
            )),
            Err(ResetRefused::Status(s)) => Err(McpError::internal_error(
                format!("set_account_password: {s}"),
                None,
            )),
        }
    }

    #[tool(
        description = "List an account's agent keys with when each was created and last used (to the hour; null when not recorded, which is every key before last-use recording began). These are the keys a compromise reset revokes. Needs admin.accounts and a recent confirmation."
    )]
    async fn list_account_agent_keys(
        &self,
        Parameters(params): Parameters<AddressParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_confirmed()?;
        crate::handlers::kevy_util::require_permission(&self.state, &user, "admin.accounts")
            .await
            .map_err(|_| McpError::invalid_request("admin.accounts permission required", None))?;
        let address = params.address.clone();
        let items = with_kevy(move |c| crate::handlers::admin_directory::key_rows(c, &address))
            .map_err(|s| McpError::internal_error(format!("list_account_agent_keys: {s}"), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({
                "items": items,
                "last_use_recorded_since": crate::handlers::admin_directory::LAST_USE_RECORDED_SINCE,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "List the permission groups an account belongs to. Scans `list_groups` and reports which ones carry the address. Admin-gated."
    )]
    async fn get_account_permissions(
        &self,
        Parameters(params): Parameters<AddressParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_route(&user, "/api/admin/accounts/{address}/groups")
            .await?;
        // Same RPC `require_admin` itself resolves through, so the
        // answer here is exactly what the gate would decide.
        let perms = self
            .state
            .core
            .effective_permissions(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("effective_permissions: {e}"), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
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
        self.require_confirmed()?;
        self.require_route(&user, "/api/admin/groups/{id}/members")
            .await?;
        let carries = crate::handlers::grants::group_permissions(params.group_id)
            .map_err(|_| McpError::internal_error("group permissions unreadable", None))?;
        crate::handlers::grants::require_may_grant(&self.state, &user, &carries)
            .await
            .map_err(|_| {
                McpError::invalid_request("that group grants what you may not grant", None)
            })?;
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
        self.require_confirmed()?;
        self.require_route(&user, "/api/admin/groups/{id}/members")
            .await?;
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

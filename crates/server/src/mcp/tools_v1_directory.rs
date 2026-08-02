//! v1 MCP tools over the directory: accounts, groups, domains, aliases,
//! apps, email groups.
//!
//! Split out of `mcp/mod.rs` on 2026-08-02, which held all sixty-one v1
//! tools in one 2,450-line file — the very thing
//! `.claude/rules/mcp-two-lane-parity.md` says not to keep doing. Tool
//! **names** are the wire contract and must match the fastcore lane;
//! `scripts/check-mcp-parity.sh` holds both to the same 82.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailMcpService;
use super::tools::*;

#[tool_router(router = tool_router_v1_directory, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(
        description = "List all email accounts. Requires admin.accounts permission. Returns address, domain, display_name, active status."
    )]
    async fn list_accounts(
        &self,
        Parameters(_params): Parameters<ListAccountsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self
            .web_state
            .domain_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let accounts = ds
            .list_accounts()
            .await
            .map_err(|e| McpError::internal_error(format!("failed to list accounts: {e}"), None))?;

        let items: Vec<serde_json::Value> = accounts
            .into_iter()
            .map(|a| {
                serde_json::json!({
                    "address": a.address,
                    "domain": a.domain,
                    "display_name": a.display_name,
                    "active": a.active,
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".into()),
        )]))
    }

    #[tool(
        description = "List all permission groups. Returns group id, name, domain (null=global), and builtin flag."
    )]
    async fn list_groups(
        &self,
        Parameters(_params): Parameters<ListGroupsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.groups")?;
        let ds = self
            .web_state
            .domain_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let groups = ds
            .list_groups(None)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        let items: Vec<serde_json::Value> = groups
            .into_iter()
            .map(|g| {
                serde_json::json!({
                    "id": g.id,
                    "name": g.name,
                    "domain": g.domain,
                    "is_builtin": g.is_builtin,
                    "description": g.description,
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".into()),
        )]))
    }

    #[tool(
        description = "Get groups and effective permissions for an account. Returns the groups the account belongs to and computed permission list."
    )]
    async fn get_account_permissions(
        &self,
        Parameters(params): Parameters<GetAccountPermissionsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.groups")?;
        let ds = self
            .web_state
            .domain_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let groups = ds
            .get_account_groups(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let perms = ds
            .load_account_permissions(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        let group_items: Vec<serde_json::Value> = groups
            .into_iter()
            .map(|g| serde_json::json!({"id": g.id, "name": g.name, "domain": g.domain}))
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "address": params.address,
                "groups": group_items,
                "permissions": perms.permission_list(),
                "accessible_domains": perms.accessible_domains(),
                "is_super": perms.is_super(),
            })
            .to_string(),
        )]))
    }

    #[tool(description = "List all managed domains. Returns domain name and creation date.")]
    async fn list_domains(
        &self,
        Parameters(_params): Parameters<ListDomainsParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let domains = ds
            .list_domains()
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = domains
            .into_iter()
            .map(|d| serde_json::json!({"name": d.name, "created_at": d.created_at}))
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "List all email aliases and forwards. Returns source, target, domain, type."
    )]
    async fn list_aliases_admin(
        &self,
        Parameters(_params): Parameters<ListAliasesAdminParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let aliases = ds
            .list_aliases()
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = aliases
            .into_iter()
            .map(|a| {
                serde_json::json!({
                    "id": a.id, "source_address": a.source_address,
                    "target_address": a.target_address, "domain": a.domain,
                    "alias_type": a.alias_type, "active": a.active,
                })
            })
            .collect();
        self.json_result(&items)
    }

    #[tool(description = "List all registered apps with their scopes and status.")]
    async fn list_apps(
        &self,
        Parameters(_params): Parameters<ListAppsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let apps = ds
            .list_apps(None)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = apps
            .into_iter()
            .map(|a| {
                serde_json::json!({
                    "app_id": a.app_id, "name": a.name, "description": a.description,
                    "scopes": a.scopes, "owner": a.owner_address, "active": a.active,
                })
            })
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "List all email groups (distribution lists). Returns group address, domain, name, member count."
    )]
    async fn list_email_groups(
        &self,
        Parameters(_params): Parameters<ListEmailGroupsParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let groups = ds
            .list_email_groups(None)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let mut items = Vec::with_capacity(groups.len());
        for g in &groups {
            let members = ds.list_email_group_members(g.id).await.unwrap_or_default();
            items.push(serde_json::json!({
                "id": g.id, "address": g.address, "domain": g.domain,
                "name": g.name, "description": g.description,
                "member_count": members.len(), "members": members,
            }));
        }
        self.json_result(&items)
    }

    #[tool(description = "List members of an email group.")]
    async fn get_email_group_members(
        &self,
        Parameters(params): Parameters<GetEmailGroupMembersParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let members = ds
            .list_email_group_members(params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> =
            members.into_iter().map(|m| serde_json::json!(m)).collect();
        self.json_result(&items)
    }
}

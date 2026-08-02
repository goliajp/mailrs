//! v1 email-group tools.
//!
//! Third slice of the `mcp/mod.rs` split (2026-08-02).
use super::MailMcpService;
use super::tools::*;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

#[tool_router(router = tool_router_v1_email_groups, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(
        description = "Create an email group (distribution list). All members receive copies of incoming mail and can reply as the group address. Requires admin.accounts permission."
    )]
    async fn create_email_group(
        &self,
        Parameters(params): Parameters<CreateEmailGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let id = ds
            .create_email_group(
                &params.address,
                &params.domain,
                &params.name,
                &params.description,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        ds.log_audit(
            &self.auth_user.address,
            "email_group_created",
            &id.to_string(),
            &format!("address={} domain={}", params.address, params.domain),
        )
        .await;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "created", "id": id, "address": params.address})
                .to_string(),
        )]))
    }

    #[tool(description = "Delete an email group by ID. Requires admin.accounts permission.")]
    async fn delete_email_group(
        &self,
        Parameters(params): Parameters<DeleteEmailGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        match ds
            .remove_email_group(params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?
        {
            Some(addr) => {
                ds.log_audit(
                    &self.auth_user.address,
                    "email_group_deleted",
                    &params.id.to_string(),
                    "",
                )
                .await;
                self.ok_result("deleted", &addr)
            }
            None => Err(McpError::invalid_params("email group not found", None)),
        }
    }

    #[tool(
        description = "Add a member to an email group. The member will receive group emails and can reply as the group address. Requires admin.accounts permission."
    )]
    async fn add_email_group_member(
        &self,
        Parameters(params): Parameters<AddEmailGroupMemberParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        ds.add_email_group_member(params.group_id, &params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        ds.log_audit(
            &self.auth_user.address,
            "email_group_member_added",
            &params.group_id.to_string(),
            &format!("address={}", params.address),
        )
        .await;
        self.ok_result("member_added", &params.address)
    }

    #[tool(
        description = "Remove a member from an email group. Requires admin.accounts permission."
    )]
    async fn remove_email_group_member(
        &self,
        Parameters(params): Parameters<RemoveEmailGroupMemberParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let removed = ds
            .remove_email_group_member(params.group_id, &params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            ds.log_audit(
                &self.auth_user.address,
                "email_group_member_removed",
                &params.group_id.to_string(),
                &format!("address={}", params.address),
            )
            .await;
            self.ok_result("member_removed", &params.address)
        } else {
            Err(McpError::invalid_params("member not found", None))
        }
    }
}

//! The caller's own drafts and templates, in their own named router.
//!
//! Split out of `tools_v1_write.rs`, same shape as the other `tools_*`
//! files: one `#[tool_router]` block, combined in `mod.rs`. Tool names
//! are the wire contract; moving a tool between files does not change one.

use super::MailrsMcpService;
use super::ok_result;
use super::params::*;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

#[tool_router(router = tool_router_v1_compose, vis = "pub")]
impl MailrsMcpService {
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
        Ok(CallToolResult::success(vec![ContentBlock::text(
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
        Ok(CallToolResult::success(vec![ContentBlock::text(
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
        Ok(CallToolResult::success(vec![ContentBlock::text(
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
        Ok(CallToolResult::success(vec![ContentBlock::text(
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
}

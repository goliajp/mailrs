//! v2 MCP tool batch 16 — labels. A label is the user's own name for
//! conversations; putting one on moves nothing, and deleting one deletes no
//! mail.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

use mailrs_core_api::method::label::{
    CreateLabelRequest, UpdateLabelRequest, label_color, label_name,
};

use super::params_labels::{
    CreateLabelParams, LabelIdParams, ThreadLabelParams, UpdateLabelParams,
};
use super::{MailrsMcpService, ok_result};

fn json(v: impl serde::Serialize) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string(&v).unwrap_or_default(),
    )])
}

fn refused(e: impl std::fmt::Display) -> McpError {
    McpError::invalid_params(e.to_string(), None)
}

#[tool_router(router = tool_router_v2_batch16, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "List your labels: id, name, colour. Use list_conversations with label=<id> for a label's conversations."
    )]
    async fn list_labels(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let items = self
            .state
            .core
            .list_labels(&user)
            .await
            .map_err(|e| McpError::internal_error(format!("list_labels: {e}"), None))?;
        Ok(json(serde_json::json!({ "items": items })))
    }

    #[tool(description = "Create a label. Returns it with its id.")]
    async fn create_label(
        &self,
        Parameters(p): Parameters<CreateLabelParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = CreateLabelRequest {
            name: label_name(&p.name).map_err(refused)?,
            color: label_color(&p.color).map_err(refused)?,
        };
        let label = self
            .state
            .core
            .create_label(&user, &req)
            .await
            .map_err(|e| refused(format!("create_label: {e}")))?;
        Ok(json(label))
    }

    #[tool(description = "Rename or recolour a label. Its conversations keep it.")]
    async fn update_label(
        &self,
        Parameters(p): Parameters<UpdateLabelParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = UpdateLabelRequest {
            name: p
                .name
                .as_deref()
                .map(label_name)
                .transpose()
                .map_err(refused)?,
            color: p
                .color
                .as_deref()
                .map(label_color)
                .transpose()
                .map_err(refused)?,
        };
        let label = self
            .state
            .core
            .update_label(&user, &p.label_id, &req)
            .await
            .map_err(|e| refused(format!("update_label: {e}")))?;
        Ok(json(label))
    }

    #[tool(description = "Delete a label. It comes off every conversation; no mail is deleted.")]
    async fn delete_label(
        &self,
        Parameters(p): Parameters<LabelIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .delete_label(&user, &p.label_id)
            .await
            .map_err(|e| refused(format!("delete_label: {e}")))?;
        Ok(ok_result())
    }

    #[tool(description = "Put a label on a conversation. The conversation stays where it is.")]
    async fn label_thread(
        &self,
        Parameters(p): Parameters<ThreadLabelParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .set_thread_label(&user, &p.thread_id, &p.label_id, true)
            .await
            .map_err(|e| refused(format!("label_thread: {e}")))?;
        Ok(ok_result())
    }

    #[tool(description = "Take a label off a conversation.")]
    async fn unlabel_thread(
        &self,
        Parameters(p): Parameters<ThreadLabelParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .set_thread_label(&user, &p.thread_id, &p.label_id, false)
            .await
            .map_err(|e| refused(format!("unlabel_thread: {e}")))?;
        Ok(ok_result())
    }
}

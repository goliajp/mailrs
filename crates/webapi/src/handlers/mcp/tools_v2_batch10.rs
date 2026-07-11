//! v2.0.0 MCP tool batch 10 — dashboard metric readouts. Small tools
//! that agents use for triage: how many unseen do I have.

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;

#[tool_router(router = tool_router_v2_batch10, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Return the caller's unseen-message count. Cheap dashboard metric — no bodies loaded. Useful for an agent deciding whether to sift or leave the inbox alone."
    )]
    async fn get_inbox_metrics(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let unseen = self
            .state
            .core
            .unseen_count(user)
            .await
            .map_err(|e| McpError::internal_error(format!("unseen_count: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "user": user,
                "unseen_count": unseen.count,
            })
            .to_string(),
        )]))
    }
}

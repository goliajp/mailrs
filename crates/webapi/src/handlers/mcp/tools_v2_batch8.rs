//! v2.0.0 MCP tool batch 8 — thread-level utility reads that agents
//! commonly want as pre-flight before send / reply.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::MailrsMcpService;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThreadIdParams {
    /// Thread ID as returned by `list_conversations`.
    pub thread_id: String,
}

// FLAG_SEEN bit shared with mailrs_mailbox — matches the flag u32
// coming back from the fastcore RPC. Kept in-file (not imported)
// so this batch file remains self-contained.
const FLAG_SEEN: u32 = 1 << 5;

#[tool_router(router = tool_router_v2_batch8, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Fetch a lightweight summary of a thread — subject, participants, message count, unread count, last_date — without loading full message bodies. Faster than read_thread for triage / list previews."
    )]
    async fn get_thread_summary(
        &self,
        Parameters(params): Parameters<ThreadIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let resp = self
            .state
            .core
            .list_thread_messages(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("list_thread_messages: {e}"), None))?;
        let count = resp.items.len();
        let participants: Vec<String> = resp
            .items
            .iter()
            .map(|w| w.sender.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let subject = resp
            .items
            .last()
            .map(|w| w.subject.clone())
            .unwrap_or_default();
        let last_date = resp
            .items
            .iter()
            .map(|w| w.internal_date)
            .max()
            .unwrap_or(0);
        let unread_count = resp
            .items
            .iter()
            .filter(|w| (w.flags & FLAG_SEEN) == 0)
            .count();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "thread_id": params.thread_id,
                "subject": subject,
                "participants": participants,
                "message_count": count,
                "unread_count": unread_count,
                "last_date": last_date,
            })
            .to_string(),
        )]))
    }
}

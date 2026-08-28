//! v2 MCP tool batch 15 — the fraud hold.
//!
//! Held mail is out of every ordinary list, so an agent that never
//! asks will never see it. That is the point of the hold and also its
//! hazard: an assistant told "did anything come from the bank" would
//! answer "no" about a message that arrived and was held. These three
//! tools are how it finds out otherwise.
//!
//! Read and release only. **Nothing here can put a conversation into
//! the hold** — that judgement is made once, at receive time, by the
//! process that saw the SMTP transaction, and a tool that could hold
//! mail on request would be a way to make somebody's mail disappear
//! by asking nicely.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use super::params::{FraudVerdictParams, ListQuarantinedParams, ReleaseQuarantinedParams};

#[tool_router(router = tool_router_v2_batch15, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "List conversations held as suspected fraud. These are kept but excluded from every other list and from its counts, so they will not appear in list_conversations or search_emails. Newest first."
    )]
    async fn list_quarantined(
        &self,
        Parameters(params): Parameters<ListQuarantinedParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let limit = params.limit.unwrap_or(20).min(200) as usize;
        let resp = self
            .state
            .core
            .list_quarantined(&user, limit, None)
            .await
            .map_err(|e| McpError::internal_error(format!("list_quarantined: {e}"), None))?;
        let items: Vec<_> = resp
            .items
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "thread_id": c.thread_id,
                    "subject": c.subject,
                    "participants": c.participants,
                    "last_date": c.last_date,
                    "snippet": c.snippet,
                })
            })
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }

    #[tool(
        description = "Read the fraud verdict stored for one message: the four layers (transport / identity / provenance / content), what each one found, the total against the threshold, and the rule-set version in force when it was decided. Returns verdict=null when nothing was found, which is not the same as examined and cleared."
    )]
    async fn get_fraud_verdict(
        &self,
        Parameters(params): Parameters<FraudVerdictParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let v = self
            .state
            .core
            .fraud_verdict(&user, &params.message_id)
            .await
            .map_err(|e| McpError::internal_error(format!("get_fraud_verdict: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
    }

    #[tool(
        description = "Release a conversation held as suspected fraud: it was not fraud. Returns it to whatever list it belonged to. The stored verdict is left alone — it records what was decided at the time."
    )]
    async fn release_quarantined(
        &self,
        Parameters(params): Parameters<ReleaseQuarantinedParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .release_quarantined(&user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("release_quarantined: {e}"), None))?;
        Ok(super::ok_result())
    }
}

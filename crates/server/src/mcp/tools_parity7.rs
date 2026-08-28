//! Parity batch 7 — the fraud hold, on the lane that does not have it.
//!
//! The hold is a `quarantined` column on kevy's `thread_user` table
//! and a verdict written by the receive path that fastcore drains.
//! This lane is the dormant pg/spg backend; it has neither, and the
//! `mcp/two-lane-parity` rule is about **tool names**, which are the
//! wire contract, not about pretending the work is done.
//!
//! So each of these three says so, out loud. An empty list would be
//! the wrong answer in the worst way: "nothing is being held" is a
//! statement about a mailbox, and this backend cannot make it. The
//! same shape as a probe pointed at nothing, which reads exactly like
//! data.
//!
//! Delete this file and implement the three properly when the pg/spg
//! backend grows the column — the tool names and their parameters are
//! already the contract, so the fastcore implementations in
//! `crates/webapi/src/handlers/mcp/tools_v2_batch15.rs` are what to
//! match.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde::Deserialize;

use super::MailMcpService;

// Read by nobody here, and that is the point: the parameter schema is
// half of the wire contract, so an agent must see the same arguments on
// both lanes even though this one can only answer with an error.
#[allow(dead_code)]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListQuarantinedParams {
    /// Max conversations (default 20, cap 200).
    #[serde(default)]
    pub limit: Option<u32>,
}

// Read by nobody here, and that is the point: the parameter schema is
// half of the wire contract, so an agent must see the same arguments on
// both lanes even though this one can only answer with an error.
#[allow(dead_code)]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct FraudVerdictParams {
    /// The message's `Message-ID`, unbracketed.
    pub message_id: String,
}

// Read by nobody here, and that is the point: the parameter schema is
// half of the wire contract, so an agent must see the same arguments on
// both lanes even though this one can only answer with an error.
#[allow(dead_code)]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ReleaseQuarantinedParams {
    /// Thread id as returned by `list_quarantined`.
    pub thread_id: String,
}

/// One sentence, said the same way three times, because a caller that
/// gets three different vague failures learns nothing.
fn no_hold_on_this_backend(tool: &str) -> McpError {
    McpError::internal_error(
        format!(
            "{tool}: the fraud hold lives on the kevy backend and this server is running the \
             pg/spg one. Nothing is claimed about whether mail is being held — this backend \
             cannot see."
        ),
        None,
    )
}

#[tool_router(router = tool_router_parity7, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(
        description = "List conversations held as suspected fraud. These are kept but excluded from every other list and from its counts, so they will not appear in list_conversations or search_emails. Newest first."
    )]
    async fn list_quarantined(
        &self,
        Parameters(_params): Parameters<ListQuarantinedParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_hold_on_this_backend("list_quarantined"))
    }

    #[tool(
        description = "Read the fraud verdict stored for one message: the four layers (transport / identity / provenance / content), what each one found, the total against the threshold, and the rule-set version in force when it was decided. Returns verdict=null when nothing was found, which is not the same as examined and cleared."
    )]
    async fn get_fraud_verdict(
        &self,
        Parameters(_params): Parameters<FraudVerdictParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_hold_on_this_backend("get_fraud_verdict"))
    }

    #[tool(
        description = "Release a conversation held as suspected fraud: it was not fraud. Returns it to whatever list it belonged to. The stored verdict is left alone — it records what was decided at the time."
    )]
    async fn release_quarantined(
        &self,
        Parameters(_params): Parameters<ReleaseQuarantinedParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_hold_on_this_backend("release_quarantined"))
    }
}

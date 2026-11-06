//! v2 MCP tool batch 19 — the ops daemon: its state and history, silences
//! and maintenance. Super administrators only; the changes also need a
//! recent confirmation. Each tool calls what the REST route calls
//! (`admin_ops_daemon::link`), so the checks and the records are the same.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use super::params_ops::{
    OpsEndSilenceParams, OpsHistoryParams, OpsMaintenanceParams, OpsSilenceParams,
};
use crate::handlers::admin_ops_daemon::link::{self, MaintenanceAction, MaintenanceRequest};

fn json(v: &serde_json::Value) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(v.to_string())])
}

fn refused(r: link::Refusal) -> McpError {
    match r.status.is_client_error() {
        true => McpError::invalid_params(r.message, None),
        false => McpError::internal_error(r.message, None),
    }
}

#[tool_router(router = tool_router_v2_batch19, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "The ops daemon's state: `reach` says whether webapi reached it (`ok`, `not_configured`, `unreachable`, `refused`); with `ok`, `stale` says whether its last completed check is older than MAILRS_OPS_SILENT_SECS, and `state` has the alerts, silences, maintenance, rules and warnings. Super-administrator only."
    )]
    async fn get_ops_state(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_super(&user).await?;
        Ok(json(&link::read_state(&user).await))
    }

    #[tool(
        description = "The ops daemon's history since `since` (unix seconds, the last day by default): alert transitions, notifications, silence and maintenance events. Super-administrator only."
    )]
    async fn get_ops_history(
        &self,
        Parameters(p): Parameters<OpsHistoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_super(&user).await?;
        let v = link::read_history(&user, p.since, p.limit)
            .await
            .map_err(refused)?;
        Ok(json(&v))
    }

    #[tool(
        description = "Silence one ops rule, or one object of it, for `for_secs` seconds (required) with a reason; the caller is recorded. Super-administrator only, with a recent confirmation."
    )]
    async fn set_ops_silence(
        &self,
        Parameters(p): Parameters<OpsSilenceParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_confirmed()?;
        self.require_super(&user).await?;
        let req = link::SilenceRequest {
            rule: p.rule,
            object: p.object,
            for_secs: p.for_secs,
            reason: p.reason,
        };
        let v = link::silence(&user, &req).await.map_err(refused)?;
        Ok(json(&v))
    }

    #[tool(
        description = "End an ops silence before its time, with a reason. Super-administrator only, with a recent confirmation."
    )]
    async fn end_ops_silence(
        &self,
        Parameters(p): Parameters<OpsEndSilenceParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_confirmed()?;
        self.require_super(&user).await?;
        let v = link::unsilence(&user, &p.id, &p.reason)
            .await
            .map_err(refused)?;
        Ok(json(&v))
    }

    #[tool(
        description = "Start, extend or end the ops daemon's maintenance state (`action`), with a reason; starting and extending need `for_secs`. While it is on, notifications are held and summed up when it ends. Super-administrator only, with a recent confirmation."
    )]
    async fn set_ops_maintenance(
        &self,
        Parameters(p): Parameters<OpsMaintenanceParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_confirmed()?;
        self.require_super(&user).await?;
        let action = match p.action.as_str() {
            "start" => MaintenanceAction::Start,
            "extend" => MaintenanceAction::Extend,
            "end" => MaintenanceAction::End,
            other => {
                return Err(McpError::invalid_params(
                    format!("action must be start, extend or end, not {other:?}"),
                    None,
                ));
            }
        };
        let req = MaintenanceRequest {
            action,
            for_secs: p.for_secs,
            reason: p.reason,
        };
        let v = link::change_maintenance(&user, &req)
            .await
            .map_err(refused)?;
        Ok(json(&v))
    }
}

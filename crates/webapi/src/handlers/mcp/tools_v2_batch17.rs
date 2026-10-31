//! v2 MCP tool batch 17 — rules: a search and what to do with the mail it
//! finds, run on mail as it arrives and, when asked, over mail already
//! received. The same functions the web's routes call.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

use crate::handlers::kevy_util::with_kevy;
use crate::handlers::rules::{Fail, RuleRequest, save};
use crate::handlers::rules_run::{preview_rows, run, undo_run};
use mailrs_core_sidestate::families::rules as stored;

use super::params_rules::{CreateRuleParams, PreviewRuleParams, RuleIdParams, RunIdParams};
use super::{MailrsMcpService, ok_result};

fn json(v: impl serde::Serialize) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string(&v).unwrap_or_default(),
    )])
}

fn refused((_, code): Fail) -> McpError {
    McpError::invalid_params(code, None)
}

#[tool_router(router = tool_router_v2_batch17, vis = "pub")]
impl MailrsMcpService {
    #[tool(description = "List your rules, in the order they run on arriving mail.")]
    async fn list_rules(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let items = with_kevy(move |c| stored::list(c, &user))
            .map_err(|_| McpError::internal_error("list_rules: store unavailable", None))?;
        Ok(json(serde_json::json!({ "items": items })))
    }

    #[tool(
        description = "Create a rule: arriving mail matching the query gets the actions. Preview first with preview_rule; run it over mail already received with apply_rule."
    )]
    async fn create_rule(
        &self,
        Parameters(p): Parameters<CreateRuleParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = RuleRequest {
            name: p.name,
            query: p.query,
            actions: p.actions,
            enabled: true,
        };
        let rule = save(&self.state, &user, None, req).await.map_err(refused)?;
        Ok(json(rule))
    }

    #[tool(description = "Delete a rule. Mail it already changed stays as it is.")]
    async fn delete_rule(
        &self,
        Parameters(p): Parameters<RuleIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let removed = with_kevy(move |c| stored::delete(c, &user, &p.rule_id))
            .map_err(|_| McpError::internal_error("delete_rule: store unavailable", None))?;
        match removed {
            true => Ok(ok_result()),
            false => Err(McpError::invalid_params("not_found", None)),
        }
    }

    #[tool(
        description = "How many conversations a rule with this query would change if run now, and the newest of them. Junk and the Trash are left out."
    )]
    async fn preview_rule(
        &self,
        Parameters(p): Parameters<PreviewRuleParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let (count, rows) = preview_rows(&self.state, &user, &p.query)
            .await
            .map_err(refused)?;
        let items: Vec<_> = rows
            .into_iter()
            .map(|c| serde_json::json!({ "thread_id": c.thread_id, "subject": c.subject }))
            .collect();
        Ok(json(serde_json::json!({ "count": count, "items": items })))
    }

    #[tool(
        description = "Run a rule over mail already received. Returns how many conversations changed and a run_id that undo_rule_run reverses for seven days."
    )]
    async fn apply_rule(
        &self,
        Parameters(p): Parameters<RuleIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let v = run(&self.state, &user, &p.rule_id).await.map_err(refused)?;
        Ok(json(v))
    }

    #[tool(description = "Put back every conversation a rule run changed. Once per run.")]
    async fn undo_rule_run(
        &self,
        Parameters(p): Parameters<RunIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let v = undo_run(&self.state, &user, &p.run_id)
            .await
            .map_err(refused)?;
        Ok(json(v))
    }
}

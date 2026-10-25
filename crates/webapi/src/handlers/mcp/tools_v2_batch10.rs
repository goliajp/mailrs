//! v2.0.0 MCP tool batch 10 — dashboard metric readouts, an account's
//! send-as grants and local forwards, and the login facts.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use super::params::{
    AccountForwardsParams, AccountSendAsParams, AuthEventsParams, SetAccountForwardsParams,
    SetAccountSendAsParams,
};

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
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({
                "user": user,
                "unseen_count": unseen.count,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "The addresses an account was granted to send as (requires admin.accounts). Its own address, the aliases pointing at it and its email groups need no grant and are not listed; get_my_permissions shows the union for the caller."
    )]
    async fn get_account_send_as(
        &self,
        Parameters(params): Parameters<AccountSendAsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        crate::handlers::kevy_util::require_permission(&self.state, &user, "admin.accounts")
            .await
            .map_err(|_| McpError::invalid_request("requires admin.accounts", None))?;
        let list = self
            .state
            .core
            .get_send_as(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("get_send_as: {e}"), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({ "address": params.address, "send_as": list.send_as }).to_string(),
        )]))
    }

    #[tool(
        description = "Replace the addresses an account may send as besides its own, its aliases and its email groups (requires admin.accounts). An empty list clears the grants. Fails for an address that is not an account or an entry that is not an address."
    )]
    async fn set_account_send_as(
        &self,
        Parameters(params): Parameters<SetAccountSendAsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = mailrs_core_api::method::admin::SendAsList {
            send_as: params.send_as,
        };
        crate::handlers::admin_directory::store_send_as(&self.state, &user, &params.address, req)
            .await
            .map_err(|code| match code {
                axum::http::StatusCode::FORBIDDEN => {
                    McpError::invalid_request("requires admin.accounts", None)
                }
                axum::http::StatusCode::NOT_FOUND => McpError::invalid_params(
                    format!("set_account_send_as: no account {}", params.address),
                    None,
                ),
                axum::http::StatusCode::BAD_REQUEST => McpError::invalid_params(
                    "set_account_send_as: an entry is not an address",
                    None,
                ),
                _ => McpError::internal_error(format!("set_account_send_as: {code}"), None),
            })?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({ "ok": true }).to_string(),
        )]))
    }

    #[tool(
        description = "The other accounts that get a copy of everything delivered to an account (requires admin.accounts)."
    )]
    async fn get_account_forwards(
        &self,
        Parameters(params): Parameters<AccountForwardsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let list =
            crate::handlers::admin_directory::load_forwards(&self.state, &user, &params.address)
                .await
                .map_err(|code| match code {
                    axum::http::StatusCode::FORBIDDEN => {
                        McpError::invalid_request("requires admin.accounts", None)
                    }
                    _ => McpError::internal_error(format!("get_account_forwards: {code}"), None),
                })?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({ "address": params.address, "forward_to": list.forward_to })
                .to_string(),
        )]))
    }

    #[tool(
        description = "Replace the accounts that get a copy of everything delivered to an account (requires admin.accounts). Each copy is written straight into the target's mailbox: nothing is sent over SMTP, the message keeps its fraud verdict, and a forwarded copy is not forwarded again. Targets must be other accounts here; an empty list stops forwarding."
    )]
    async fn set_account_forwards(
        &self,
        Parameters(params): Parameters<SetAccountForwardsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = crate::handlers::admin_directory::LocalForwards {
            forward_to: params.forward_to,
        };
        crate::handlers::admin_directory::store_forwards(&self.state, &user, &params.address, req)
            .await
            .map_err(|code| match code {
                axum::http::StatusCode::FORBIDDEN => {
                    McpError::invalid_request("requires admin.accounts", None)
                }
                axum::http::StatusCode::NOT_FOUND => McpError::invalid_params(
                    format!("set_account_forwards: no account {}", params.address),
                    None,
                ),
                axum::http::StatusCode::BAD_REQUEST => McpError::invalid_params(
                    "set_account_forwards: every target must be another account here",
                    None,
                ),
                _ => McpError::internal_error(format!("set_account_forwards: {code}"), None),
            })?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({ "ok": true }).to_string(),
        )]))
    }

    #[tool(
        description = "Sign-in attempts on every way in (web, verify, IMAP, POP3, SMTP, ManageSieve, DAV, API keys), newest first, filtered by account, source address, outcome, channel and time (requires admin.accounts). Each fact has occurred_at, account, source_ip, channel, credential, result and, for keys, key_id."
    )]
    async fn list_auth_events(
        &self,
        Parameters(params): Parameters<AuthEventsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let query: mailrs_auth_guard::events::Query = serde_json::to_value(&params)
            .and_then(serde_json::from_value)
            .map_err(|e| McpError::invalid_params(format!("list_auth_events: {e}"), None))?;
        let items = crate::handlers::admin_auth_events::auth_events(&self.state, &user, query)
            .await
            .map_err(|code| match code {
                axum::http::StatusCode::FORBIDDEN => {
                    McpError::invalid_request("requires admin.accounts", None)
                }
                _ => McpError::internal_error(format!("list_auth_events: {code}"), None),
            })?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }
}

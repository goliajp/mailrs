//! Disposable test mailboxes for agents: make an address, use it to sign
//! up somewhere, wait for the mail, read the code out of it.
//!
//! Same operations as `/api/mock/*`, through the same service.

use std::time::Duration;

use mailrs_core_sidestate::families::mock::{Filter, MAX_LISTING};
use mailrs_mock::tool_params::{
    MockCreateNamespaceParams, MockDeleteMessagesParams, MockListMessagesParams,
    MockListNamespacesParams, MockNamespaceParams, MockNewAddressParams, MockReadMessageParams,
    MockSendParams, MockWaitForMessageParams,
};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use crate::handlers::mock::messages;
use crate::handlers::mock::send::{self, SendRequest};
use crate::handlers::mock::service::{self, MockApiError, MockCaller};

fn json(value: impl serde::Serialize) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string(&value)
        .map_err(|e| McpError::internal_error(format!("encode: {e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn failed(e: MockApiError) -> McpError {
    match e.status.is_client_error() {
        true => McpError::invalid_params(e.message, None),
        false => McpError::internal_error(e.message, None),
    }
}

impl MailrsMcpService {
    fn mock_caller(&self) -> Result<MockCaller, McpError> {
        let user = self.require_user()?.to_string();
        Ok(MockCaller {
            user,
            only_namespaces: self
                .identity
                .scopes
                .as_deref()
                .and_then(crate::session::mock_ns_restriction),
        })
    }
}

#[tool_router(router = tool_router_mock, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Create a mock-mail namespace: a disposable test mailbox that receives mail for every address {namespace}.{any-tag}@<mock domain>. Mock mail never touches real mailboxes and nothing is ever sent outward from it. Kept 3 days, 500 messages per namespace, 10 namespaces per user."
    )]
    async fn mock_create_namespace(
        &self,
        Parameters(p): Parameters<MockCreateNamespaceParams>,
    ) -> Result<CallToolResult, McpError> {
        let caller = self.mock_caller()?;
        json(
            service::create_namespace(&self.state, &caller, &p.name)
                .await
                .map_err(failed)?,
        )
    }

    #[tool(description = "List your mock-mail namespaces with their message counts.")]
    async fn mock_list_namespaces(
        &self,
        Parameters(p): Parameters<MockListNamespacesParams>,
    ) -> Result<CallToolResult, McpError> {
        let caller = self.mock_caller()?;
        let all = p.all.unwrap_or(false);
        json(
            service::list_namespaces(&self.state, &caller, all)
                .await
                .map_err(failed)?,
        )
    }

    #[tool(description = "Delete a mock-mail namespace and every message in it.")]
    async fn mock_delete_namespace(
        &self,
        Parameters(p): Parameters<MockNamespaceParams>,
    ) -> Result<CallToolResult, McpError> {
        let caller = self.mock_caller()?;
        service::delete_namespace(&self.state, &caller, &p.namespace)
            .await
            .map_err(failed)?;
        Ok(super::ok_result())
    }

    #[tool(
        description = "Hand out a fresh address in a mock namespace. Use it to sign up somewhere, then call mock_wait_for_message with the same tag: mail that arrived since this call counts."
    )]
    async fn mock_new_address(
        &self,
        Parameters(p): Parameters<MockNewAddressParams>,
    ) -> Result<CallToolResult, McpError> {
        let caller = self.mock_caller()?;
        let minted = service::new_address(&self.state, &caller, &p.namespace, p.tag.as_deref())
            .await
            .map_err(failed)?;
        json(minted)
    }

    #[tool(
        description = "List messages in a mock namespace, newest first: sender, subject, extracted one-time codes and links, attachments. Bodies are left out; use mock_read_message for them."
    )]
    async fn mock_list_messages(
        &self,
        Parameters(p): Parameters<MockListMessagesParams>,
    ) -> Result<CallToolResult, McpError> {
        let caller = self.mock_caller()?;
        let filter = Filter {
            tag: p.tag,
            tag_prefix: p.tag_prefix,
            to: p.to,
            from: p.from,
            subject: p.subject,
            since: p.since,
            limit: p.limit.unwrap_or(20).clamp(1, MAX_LISTING as u32) as usize,
        };
        json(
            messages::list_messages(&self.state, &caller, &p.namespace, filter)
                .await
                .map_err(failed)?,
        )
    }

    #[tool(
        description = "Wait up to 60 s for the next message in a mock namespace and return it whole, with `codes` (verification codes found in it) and `links` (verify / confirm links first). Returns {\"status\":\"timeout\"} if none arrives."
    )]
    async fn mock_wait_for_message(
        &self,
        Parameters(p): Parameters<MockWaitForMessageParams>,
    ) -> Result<CallToolResult, McpError> {
        let caller = self.mock_caller()?;
        let filter = Filter {
            tag: p.tag,
            from: p.from,
            subject: p.subject,
            since: p.since,
            ..Default::default()
        };
        let timeout = p.timeout_secs.map(Duration::from_secs);
        json(
            messages::wait_for_message(&self.state, &caller, &p.namespace, filter, timeout)
                .await
                .map_err(failed)?,
        )
    }

    #[tool(
        description = "Read one mock message whole: headers, text and HTML bodies, codes and links."
    )]
    async fn mock_read_message(
        &self,
        Parameters(p): Parameters<MockReadMessageParams>,
    ) -> Result<CallToolResult, McpError> {
        let caller = self.mock_caller()?;
        json(
            messages::read_message(&self.state, &caller, &p.namespace, &p.id)
                .await
                .map_err(failed)?,
        )
    }

    #[tool(
        description = "Delete one mock message, the messages of one tag, or all messages of a namespace."
    )]
    async fn mock_delete_messages(
        &self,
        Parameters(p): Parameters<MockDeleteMessagesParams>,
    ) -> Result<CallToolResult, McpError> {
        let caller = self.mock_caller()?;
        let removed = messages::delete_messages(
            &self.state,
            &caller,
            &p.namespace,
            p.id.as_deref(),
            p.tag.as_deref(),
        )
        .await
        .map_err(failed)?;
        json(serde_json::json!({ "deleted": removed }))
    }

    #[tool(
        description = "Send a message from a mock namespace. Mock recipients receive it; any other recipient is recorded in the namespace's captured copy and is never contacted."
    )]
    async fn mock_send(
        &self,
        Parameters(p): Parameters<MockSendParams>,
    ) -> Result<CallToolResult, McpError> {
        let caller = self.mock_caller()?;
        let req = SendRequest {
            from_tag: p.from_tag,
            to: p.to,
            subject: p.subject,
            text: p.text,
            html: p.html,
        };
        json(
            send::send(&self.state, &caller, &p.namespace, req)
                .await
                .map_err(failed)?,
        )
    }
}

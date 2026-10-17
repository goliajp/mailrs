//! The `mock_*` tools, on the lane that has no mock store.
//!
//! Mock mail lives in the network kevy and in files fastcore writes; this
//! dormant pg/spg lane serves neither. The tool names and their arguments
//! are the wire contract both lanes share, so they are registered here and
//! each one says plainly that this deployment cannot answer.

use mailrs_mock::tool_params::{
    MockCreateNamespaceParams, MockDeleteMessagesParams, MockListMessagesParams,
    MockListNamespacesParams, MockNamespaceParams, MockNewAddressParams, MockReadMessageParams,
    MockSendParams, MockWaitForMessageParams,
};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};

use super::MailMcpService;

fn no_mock_store(tool: &str) -> McpError {
    McpError::internal_error(
        format!(
            "{tool}: this deployment has no mock store; mock mail is served by the kevy backend"
        ),
        None,
    )
}

#[tool_router(router = tool_router_mock, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(description = "Create a mock-mail namespace. Not available on this backend.")]
    async fn mock_create_namespace(
        &self,
        Parameters(_p): Parameters<MockCreateNamespaceParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_mock_store("mock_create_namespace"))
    }

    #[tool(description = "List your mock-mail namespaces. Not available on this backend.")]
    async fn mock_list_namespaces(
        &self,
        Parameters(_p): Parameters<MockListNamespacesParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_mock_store("mock_list_namespaces"))
    }

    #[tool(description = "Delete a mock-mail namespace. Not available on this backend.")]
    async fn mock_delete_namespace(
        &self,
        Parameters(_p): Parameters<MockNamespaceParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_mock_store("mock_delete_namespace"))
    }

    #[tool(description = "Hand out a fresh mock address. Not available on this backend.")]
    async fn mock_new_address(
        &self,
        Parameters(_p): Parameters<MockNewAddressParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_mock_store("mock_new_address"))
    }

    #[tool(description = "List mock messages. Not available on this backend.")]
    async fn mock_list_messages(
        &self,
        Parameters(_p): Parameters<MockListMessagesParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_mock_store("mock_list_messages"))
    }

    #[tool(description = "Wait for a mock message. Not available on this backend.")]
    async fn mock_wait_for_message(
        &self,
        Parameters(_p): Parameters<MockWaitForMessageParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_mock_store("mock_wait_for_message"))
    }

    #[tool(description = "Read one mock message. Not available on this backend.")]
    async fn mock_read_message(
        &self,
        Parameters(_p): Parameters<MockReadMessageParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_mock_store("mock_read_message"))
    }

    #[tool(description = "Delete mock messages. Not available on this backend.")]
    async fn mock_delete_messages(
        &self,
        Parameters(_p): Parameters<MockDeleteMessagesParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_mock_store("mock_delete_messages"))
    }

    #[tool(description = "Send from a mock namespace. Not available on this backend.")]
    async fn mock_send(
        &self,
        Parameters(_p): Parameters<MockSendParams>,
    ) -> Result<CallToolResult, McpError> {
        Err(no_mock_store("mock_send"))
    }
}

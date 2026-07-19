//! v2 MCP tool batch 12 — app registration + email distribution
//! groups. Two-lane parity with the monolith's `create_app` /
//! `delete_app` / `create_email_group` / `delete_email_group` /
//! `add_email_group_member` / `remove_email_group_member`.
//!
//! Every tool delegates to the fastcore REST handler that already owns
//! the kevy layout (`handlers::complete` for apps + groups,
//! `handlers::admin` for group membership) so the MCP surface and the
//! admin UI can never drift apart.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::params::{
    AppIdParams, CreateAppParams, CreateEmailGroupParams, EmailGroupIdNumParams,
    EmailGroupMemberParams,
};
use super::{MailrsMcpService, ok_result};
use crate::handlers::conversations::AuthedUser;

fn map_status(code: StatusCode, what: &str) -> McpError {
    if code == StatusCode::NOT_FOUND {
        return McpError::invalid_params(format!("{what}: not found"), None);
    }
    McpError::internal_error(format!("{what}: {code}"), None)
}

#[tool_router(router = tool_router_v2_batch12, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Register a new app and mint its client secret. The secret is returned once and only its sha256 is stored — save it now. Admin-gated."
    )]
    async fn create_app(
        &self,
        Parameters(params): Parameters<CreateAppParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let req = crate::handlers::complete::CreateAppRequest {
            name: params.name.clone(),
            scopes: params.scopes,
        };
        let Json(app) = crate::handlers::complete::create_app(Json(req))
            .await
            .map_err(|c| map_status(c, "create_app"))?;
        crate::handlers::audit::record(&user, "app.create", &params.name, "via mcp");
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "app": app,
                "warning": "Save the secret now. Only its sha256 is stored.",
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Delete a registered app by app_id. Admin-gated.")]
    async fn delete_app(
        &self,
        Parameters(params): Parameters<AppIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        crate::handlers::complete::delete_app(Path(params.app_id.clone()))
            .await
            .map_err(|c| map_status(c, "delete_app"))?;
        crate::handlers::audit::record(&user, "app.delete", &params.app_id, "via mcp");
        Ok(ok_result())
    }

    #[tool(
        description = "Create an email distribution group. Members receive copies of mail sent to the group address. Admin-gated."
    )]
    async fn create_email_group(
        &self,
        Parameters(params): Parameters<CreateEmailGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let req = crate::handlers::complete::CreateEmailGroupRequest {
            address: params.address.clone(),
            name: params.name,
            members: params.members,
        };
        let Json(group) = crate::handlers::complete::create_email_group(Json(req))
            .await
            .map_err(|c| map_status(c, "create_email_group"))?;
        crate::handlers::audit::record(&user, "email_group.create", &params.address, "via mcp");
        Ok(CallToolResult::success(vec![Content::text(
            group.to_string(),
        )]))
    }

    #[tool(description = "Delete an email distribution group by id. Admin-gated.")]
    async fn delete_email_group(
        &self,
        Parameters(params): Parameters<EmailGroupIdNumParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        crate::handlers::complete::delete_email_group(Path(params.id))
            .await
            .map_err(|c| map_status(c, "delete_email_group"))?;
        crate::handlers::audit::record(
            &user,
            "email_group.delete",
            &params.id.to_string(),
            "via mcp",
        );
        Ok(ok_result())
    }

    #[tool(
        description = "Add a member address to an email distribution group. Admin-gated. Use `get_email_group_members` to read the current roster."
    )]
    async fn add_email_group_member(
        &self,
        Parameters(params): Parameters<EmailGroupMemberParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let req = crate::handlers::admin::AddMemberRequest {
            address: params.address,
        };
        crate::handlers::admin::add_email_group_member(
            State(self.state.clone()),
            axum::Extension(AuthedUser(user)),
            Path(params.group_id),
            Json(req),
        )
        .await
        .map_err(|c| map_status(c, "add_email_group_member"))?;
        Ok(ok_result())
    }

    #[tool(description = "Remove a member address from an email distribution group. Admin-gated.")]
    async fn remove_email_group_member(
        &self,
        Parameters(params): Parameters<EmailGroupMemberParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        crate::handlers::admin::remove_email_group_member(
            State(self.state.clone()),
            axum::Extension(AuthedUser(user)),
            Path((params.group_id, params.address)),
        )
        .await
        .map_err(|c| map_status(c, "remove_email_group_member"))?;
        Ok(ok_result())
    }
}

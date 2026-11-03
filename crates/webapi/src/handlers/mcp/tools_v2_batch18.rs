//! v2 MCP tool batch 18 — a reader's rules for senders and their push
//! settings. Each tool calls the handler the web's route calls, so the
//! checks and the stored facts are the same.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

use super::params_senders::{
    AcceptSendersParams, CorrectionIdParams, ImportantContactParams, NotifySettingsParams,
    NotifyStatsParams, ReclassifySenderParams, SenderAddressParams, SenderNotifyParams,
    SenderRuleParams, ThreadNotifyParams,
};
use super::{MailrsMcpService, ok_result};
use crate::handlers::conversations::AuthedUser;
use crate::handlers::{notify, sender_rules, senders};

fn json(v: impl serde::Serialize) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string(&v).unwrap_or_default(),
    )])
}

fn map_status(code: StatusCode, what: &str) -> McpError {
    match code {
        StatusCode::NOT_FOUND => McpError::invalid_params(format!("{what}: not found"), None),
        StatusCode::BAD_REQUEST => McpError::invalid_params(format!("{what}: invalid"), None),
        _ => McpError::internal_error(format!("{what}: {code}"), None),
    }
}

/// A value the web would send as JSON, read the way the route reads it.
fn from_json<T: serde::de::DeserializeOwned>(
    v: serde_json::Value,
    what: &str,
) -> Result<T, McpError> {
    serde_json::from_value(v).map_err(|e| McpError::invalid_params(format!("{what}: {e}"), None))
}

#[tool_router(router = tool_router_v2_batch18, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Move every conversation from a sender to inbox, notifications or promotions, and remember it for their future mail. Returns how many moved and a correction_id that undo_sender_correction reverses."
    )]
    async fn reclassify_sender(
        &self,
        Parameters(p): Parameters<ReclassifySenderParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let body = sender_rules::ReclassifyBody {
            sender: p.sender,
            bucket: p.bucket,
        };
        let Json(a) = sender_rules::reclassify(
            State(self.state.clone()),
            axum::Extension(AuthedUser(user)),
            Json(body),
        )
        .await
        .map_err(|c| map_status(c, "reclassify_sender"))?;
        Ok(json(a))
    }

    #[tool(
        description = "Put back the conversations a reclassify_sender moved, and drop the sender's remembered bucket."
    )]
    async fn undo_sender_correction(
        &self,
        Parameters(p): Parameters<CorrectionIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let Json(a) = sender_rules::undo(
            State(self.state.clone()),
            axum::Extension(AuthedUser(user)),
            Path(p.correction_id),
        )
        .await
        .map_err(|c| map_status(c, "undo_sender_correction"))?;
        Ok(json(a))
    }

    #[tool(
        description = "List your sender rules: the bucket each sender's mail goes to and how long it is kept."
    )]
    async fn list_sender_rules(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let Json(rows) = sender_rules::list(axum::Extension(AuthedUser(user)))
            .await
            .map_err(|c| map_status(c, "list_sender_rules"))?;
        Ok(json(serde_json::json!({ "items": rows })))
    }

    #[tool(
        description = "Set a rule for one sender: the bucket for their mail, and how much of it to keep (keep_latest newest, or older_than_days). A keeping rule applies at once; returns how many conversations it let go of."
    )]
    async fn set_sender_rule(
        &self,
        Parameters(p): Parameters<SenderRuleParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let body: sender_rules::RuleBody = from_json(
            serde_json::json!({
                "bucket": p.bucket,
                "keep_latest": p.keep_latest,
                "older_than_days": p.older_than_days,
                "disposal": p.disposal.unwrap_or_else(|| "archive".into()),
            }),
            "set_sender_rule",
        )?;
        let Json(a) = sender_rules::put(
            State(self.state.clone()),
            axum::Extension(AuthedUser(user)),
            Path(p.address),
            Json(body),
        )
        .await
        .map_err(|c| map_status(c, "set_sender_rule"))?;
        Ok(json(a))
    }

    #[tool(
        description = "Remove the rule for one sender. Mail it already moved stays where it is."
    )]
    async fn delete_sender_rule(
        &self,
        Parameters(p): Parameters<SenderAddressParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        sender_rules::delete(axum::Extension(AuthedUser(user)), Path(p.address))
            .await
            .map_err(|c| map_status(c, "delete_sender_rule"))?;
        Ok(ok_result())
    }

    #[tool(
        description = "Your push settings: which mail pushes, quiet hours, and the per-sender and per-conversation choices."
    )]
    async fn get_notify_settings(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let Json(prefs) = notify::get_prefs(axum::Extension(AuthedUser(user.clone())))
            .await
            .map_err(|c| map_status(c, "get_notify_settings"))?;
        let Json(overrides) = notify::overrides(axum::Extension(AuthedUser(user)))
            .await
            .map_err(|c| map_status(c, "get_notify_settings"))?;
        Ok(json(
            serde_json::json!({ "prefs": prefs, "overrides": overrides }),
        ))
    }

    #[tool(description = "Set which mail pushes to your devices, and quiet hours.")]
    async fn set_notify_settings(
        &self,
        Parameters(p): Parameters<NotifySettingsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let prefs = from_json(
            serde_json::json!({
                "mode": p.mode,
                "quiet_start": p.quiet_start,
                "quiet_end": p.quiet_end,
                "utc_offset_minutes": p.utc_offset_minutes,
                "hide_content": p.hide_content,
            }),
            "set_notify_settings",
        )?;
        notify::put_prefs(axum::Extension(AuthedUser(user)), Json(prefs))
            .await
            .map_err(|c| map_status(c, "set_notify_settings"))?;
        Ok(ok_result())
    }

    #[tool(
        description = "Always or never push mail from one sender, whatever the mode; no value removes the choice."
    )]
    async fn set_sender_notify(
        &self,
        Parameters(p): Parameters<SenderNotifyParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        notify::put_sender(
            axum::Extension(AuthedUser(user)),
            Path(p.address),
            Json(notify::OverrideBody { value: p.value }),
        )
        .await
        .map_err(|c| map_status(c, "set_sender_notify"))?;
        Ok(ok_result())
    }

    #[tool(description = "Follow or mute one conversation's pushes; no value removes the choice.")]
    async fn set_thread_notify(
        &self,
        Parameters(p): Parameters<ThreadNotifyParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        notify::put_thread(
            axum::Extension(AuthedUser(user)),
            Path(p.thread_id),
            Json(notify::OverrideBody { value: p.value }),
        )
        .await
        .map_err(|c| map_status(c, "set_thread_notify"))?;
        Ok(ok_result())
    }

    #[tool(
        description = "How many pushes were sent, held back by your settings or quiet hours, failed, or opened, per day, newest first."
    )]
    async fn notify_stats(
        &self,
        Parameters(p): Parameters<NotifyStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let q = notify::StatsQuery {
            days: p.days.unwrap_or(7),
        };
        let Json(days) = notify::stats(axum::Extension(AuthedUser(user)), Query(q))
            .await
            .map_err(|c| map_status(c, "notify_stats"))?;
        Ok(json(serde_json::json!({ "items": days })))
    }

    #[tool(description = "Your important contacts: their mail pushes in the default mode.")]
    async fn list_important_contacts(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let Json(v) = notify::important(axum::Extension(AuthedUser(user)))
            .await
            .map_err(|c| map_status(c, "list_important_contacts"))?;
        Ok(json(serde_json::json!({ "items": v })))
    }

    #[tool(description = "Mark a contact important, or remove the mark.")]
    async fn set_important_contact(
        &self,
        Parameters(p): Parameters<ImportantContactParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        notify::put_important(
            axum::Extension(AuthedUser(user)),
            Path(p.address),
            Json(notify::ImportantBody {
                important: p.important,
            }),
        )
        .await
        .map_err(|c| map_status(c, "set_important_contact"))?;
        Ok(ok_result())
    }

    #[tool(
        description = "Accept senders you hear from for the first time: their conversations leave the New section of the inbox. Returns how many distinct addresses were recorded."
    )]
    async fn accept_senders(
        &self,
        Parameters(p): Parameters<AcceptSendersParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let Json(a) = senders::accept_senders(
            State(self.state.clone()),
            axum::Extension(AuthedUser(user)),
            Json(senders::AcceptBody {
                addresses: p.addresses,
            }),
        )
        .await
        .map_err(|c| map_status(c, "accept_senders"))?;
        Ok(json(a))
    }
}

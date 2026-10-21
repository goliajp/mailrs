//! Shared helpers used across multiple `mail/` sub-modules.
//!
//! Anything in this file is reachable from sibling sub-modules via
//! `use super::common::*`. Items that need to remain callable from outside
//! the `mail::` module (e.g. `mcp/mod.rs`, `web/rsvp.rs`, `web/auth.rs`,
//! `web/jmap.rs`) are `pub(crate)` so the `pub(crate) use common::*` re-export
//! in `mod.rs` lifts them to the `mail::` path.

use std::sync::Arc;

use axum::Json;

use super::{SendResult, WebState};

mod build;
mod deliver;

pub(crate) use build::*;
pub(crate) use deliver::deliver_message_ex;

/// Attachment payload used by the send pipeline (multipart upload + forwarded
/// attachments + MCP send-with-attachments). Lives in `common.rs` because it
/// is shared by `send.rs` (handlers) and `crate::mcp::mod` (MCP tools).
pub(crate) struct AttachmentData {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// check if a sender address is allowed for the authenticated user
/// returns Ok(()) if allowed, Err(message) if not
pub(crate) fn verify_sender(
    from: &str,
    user: &str,
    permissions: &crate::permission::EffectivePermissions,
) -> Result<(), &'static str> {
    if from == user {
        return Ok(());
    }
    // check if from is an alias address owned by this user
    if permissions
        .send_as()
        .iter()
        .any(|a| a.eq_ignore_ascii_case(from))
    {
        return Ok(());
    }
    // super user or user with accessible domains
    let accessible = permissions.accessible_domains();
    if !accessible.is_empty()
        && let Some(domain) = from.rsplit_once('@').map(|(_, d)| d)
        && (permissions.is_super() || accessible.iter().any(|sd| sd.eq_ignore_ascii_case(domain)))
    {
        return Ok(());
    }
    Err("sender must match authenticated user")
}

/// resolve reply_to_thread_id into in_reply_to message-id and references
/// returns (resolved_in_reply_to, references)
pub(crate) async fn resolve_thread_reply(
    reply_to_thread_id: Option<&str>,
    in_reply_to: Option<&str>,
    user: &str,
    mb_store: Option<&mailrs_mailbox::PgMailboxStore>,
) -> (Option<String>, Vec<String>) {
    // explicit in_reply_to takes precedence
    if let Some(reply_to) = in_reply_to
        && !reply_to.is_empty()
    {
        let refs = match mb_store {
            Some(store) => store
                .get_thread_references(user, reply_to)
                .await
                .unwrap_or_default(),
            None => vec![],
        };
        return (Some(reply_to.to_string()), refs);
    }

    // resolve thread_id to last message's message-id
    if let (Some(thread_id), Some(store)) = (reply_to_thread_id, mb_store)
        && !thread_id.is_empty()
        && let Ok(Some(last_msg_id)) = store.get_last_message_id_in_thread(user, thread_id).await
    {
        let refs = store
            .get_thread_message_ids(user, thread_id)
            .await
            .unwrap_or_default();
        return (Some(last_msg_id), refs);
    }

    (None, vec![])
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn deliver_message(
    state: &Arc<WebState>,
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    raw: &[u8],
    message_id: &str,
    ts: i64,
) -> Json<SendResult> {
    deliver_message_ex(state, from, to, cc, bcc, raw, message_id, ts, None).await
}

// extract bare email from "Display Name <addr>" or return as-is
pub(super) fn extract_address(s: &str) -> String {
    if let Some(start) = s.rfind('<')
        && let Some(end) = s[start..].find('>')
    {
        return s[start + 1..start + end].trim().to_string();
    }
    s.trim().to_string()
}

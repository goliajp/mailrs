//! Outbound send pipeline: JSON send, multipart send (with attachments),
//! deliverability checks, and cancel-pending-send.

use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};


use super::{ApiResult, AuthUser, WebState};

mod multipart;
mod text;

pub(crate) use multipart::send_message_multipart;
pub(crate) use text::send_message;


#[derive(Deserialize)]
pub(crate) struct SendMessageRequest {
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub html_body: Option<String>,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub reply_to_thread_id: Option<String>,
    #[serde(default)]
    pub list_unsubscribe: Option<String>,
    /// optional ISO 8601 timestamp for scheduled delivery
    #[serde(default)]
    pub scheduled_at: Option<String>,
    /// request a read receipt (MDN) from recipients
    #[serde(default)]
    pub request_read_receipt: bool,
    /// uid of original message to forward attachments from (legacy, prefer forward_message_id)
    #[serde(default)]
    pub forward_attachments_from: Option<u32>,
    /// message-id header of original message to forward (more reliable than uid)
    #[serde(default)]
    pub forward_message_id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct DeliverabilityCheckRequest {
    pub recipient: String,
}

#[derive(Serialize)]
pub(crate) struct DeliverabilityCheckResult {
    pub recipient: String,
    pub suppressed: bool,
    pub mx_found: bool,
    pub mx_hosts: Vec<String>,
    pub issues: Vec<String>,
}


pub(crate) async fn cancel_pending_send(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    axum::extract::Path(message_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if message_id.is_empty() || message_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid message_id".into()),
        });
    }

    let Some(ref pool) = state.outbound_queue else {
        return Json(ApiResult {
            success: false,
            message: Some("outbound queue not configured".into()),
        });
    };

    match mailrs_outbound_queue::queue::cancel_pending_by_message_id(pool, &message_id, &user).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("message not found or already sent".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(format!("failed to cancel: {e}")),
        }),
    }
}


pub(crate) async fn check_deliverability(
    _auth: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<DeliverabilityCheckRequest>,
) -> impl IntoResponse {
    let mut issues = Vec::new();
    let recipient = req.recipient.trim().to_lowercase();

    // check suppression list
    let suppressed = if let Some(ref pool) = state.outbound_queue {
        mailrs_outbound_queue::queue::is_suppressed(pool, &recipient).await
    } else {
        false
    };
    if suppressed {
        issues.push("recipient is on suppression list (previous hard bounce)".into());
    }

    // check MX records
    let domain = recipient.split_once('@').map(|(_, d)| d).unwrap_or("");
    let (mx_found, mx_hosts) = if let Some(ref resolver) = state.resolver {
        match mailrs_smtp_client::resolve_mx(resolver, domain).await {
            Ok(records) => (true, records.iter().map(|r| r.exchange.clone()).collect()),
            Err(e) => {
                issues.push(format!("MX lookup failed: {e}"));
                (false, vec![])
            }
        }
    } else {
        issues.push("DNS resolver not available".into());
        (false, vec![])
    };

    if domain.is_empty() {
        issues.push("invalid email address".into());
    }

    Json(DeliverabilityCheckResult {
        recipient,
        suppressed,
        mx_found,
        mx_hosts,
        issues,
    })
}

pub(super) async fn resolve_inline_images(
    html: &str,
    maildir_root: &str,
    user_address: &str,
    hostname: &str,
) -> (String, Vec<crate::inline_image::InlineImage>) {
    let ids = crate::inline_image::find_inline_urls(html);
    if ids.is_empty() {
        return (html.to_string(), vec![]);
    }

    let mut images = Vec::new();
    for id in &ids {
        // try all known extensions
        for ext in &["png", "jpg", "webp", "gif", "tiff", "bmp", "svg", "bin"] {
            let path =
                crate::inline_image::inline_path(maildir_root, user_address, id, ext);
            if let Ok(data) = tokio::fs::read(&path).await {
                let content_type = match *ext {
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "webp" => "image/webp",
                    "gif" => "image/gif",
                    "tiff" => "image/tiff",
                    "bmp" => "image/bmp",
                    "svg" => "image/svg+xml",
                    _ => "application/octet-stream",
                };
                images.push(crate::inline_image::InlineImage {
                    id: id.clone(),
                    content_type: content_type.to_string(),
                    data,
                    cid: format!("{id}@{hostname}"),
                });
                break;
            }
        }
    }

    let rewritten = crate::inline_image::replace_inline_urls_with_cid(html, &images);
    (rewritten, images)
}


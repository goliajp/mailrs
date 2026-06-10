//! Per-message reads + flag updates + delete.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::message_util;

use super::{ApiResult, AuthUser, WebState};

#[derive(Serialize)]
pub(crate) struct MessageDetail {
    pub uid: u32,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub size: u32,
    pub flags: u32,
    pub internal_date: i64,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub attachments: Vec<crate::message_util::AttachmentInfo>,
    pub category: String,
    pub risk_score: u8,
    pub risk_reason: String,
    pub summary: String,
    pub people: serde_json::Value,
    pub dates: serde_json::Value,
    pub amounts: serde_json::Value,
    pub action_items: serde_json::Value,
    pub ai_analyzed: bool,
    pub clean_text: Option<String>,
    /// Parsed iTIP invite payload (RFC 5545 / 5546). Present when the
    /// message carried a `text/calendar` MIME part that mailrs::ical
    /// could parse (filled by the inbound pipeline, MRS-4).
    pub invite_payload: Option<serde_json::Value>,
    /// METHOD value if the message is an invite (REQUEST / REPLY /
    /// CANCEL / UPDATE / ...); None for non-invite messages.
    pub invite_method: Option<String>,
    /// MRS-19: user's RSVP partstat for this invite if previously sent
    /// (ACCEPTED / TENTATIVE / DECLINED). NULL means "not replied yet".
    pub rsvp_status: Option<String>,
    /// Timestamp the RSVP partstat above was recorded.
    pub rsvp_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
pub(crate) struct FlagUpdate {
    pub action: String,
    pub flags: u32,
}

/// MRS-14 hotfix: on-read backfill of `messages.invite_payload` /
/// `invite_method` for messages delivered before MRS-4 (v1.5.0) shipped.
///
/// When the stored row has NULL for these columns and we have the raw
/// maildir bytes in hand anyway (the surrounding handler reads them to
/// render the body), parse the `text/calendar` part now via
/// `crate::calendar::invite_extract` + `mailrs_ical::parse_invite` and
/// `UPDATE messages` so the next read hits the populated columns.
///
/// Returns the freshly-parsed (payload_json, method_str) tuple on success;
/// None when no invite part is found, parsing fails, or the SQL update
/// fails. None lets the caller fall back to the stored (null) values.
async fn try_lazy_backfill_invite(
    pool: &crate::pg::BackendPool,
    message_id: i64,
    raw_bytes: &[u8],
) -> Option<(serde_json::Value, String)> {
    let extracted = crate::calendar::invite_extract::extract_invite_part(raw_bytes)?;
    let parsed = mailrs_ical::parse_invite(&extracted.ics_bytes).ok()?;
    let payload_json = serde_json::to_value(&parsed).ok()?;
    let method_str = format!("{:?}", parsed.method).to_uppercase();

    if let Err(e) =
        sqlx::query("UPDATE messages SET invite_payload = $1, invite_method = $2 WHERE id = $3")
            .bind(&payload_json)
            .bind(&method_str)
            .bind(message_id)
            .execute(pool)
            .await
    {
        tracing::warn!("lazy invite backfill UPDATE failed for {message_id}: {e}");
        return None;
    }

    Some((payload_json, method_str))
}

pub(crate) async fn get_message(
    Path(uid): Path<u32>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(None::<MessageDetail>);
    };

    // find the message across all mailboxes
    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await {
            let raw =
                message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id).await;
            let parsed = raw
                .as_deref()
                .map(message_util::parse_message)
                .unwrap_or_default();
            let sender = message_util::decode_header(&msg.sender);
            let subject = message_util::decode_header(&msg.subject);

            // try AI analysis first, fall back to rule-based
            let ai = mb_store.get_email_analysis(msg.id).await.ok().flatten();
            let (
                category,
                risk_score,
                risk_reason,
                summary,
                people,
                dates,
                amounts,
                action_items,
                ai_analyzed,
                clean_text,
            ) = if let Some(ref a) = ai {
                let ct = if a.clean_text.is_empty() {
                    None
                } else {
                    Some(a.clean_text.clone())
                };
                (
                    a.category.clone(),
                    a.risk_score as u8,
                    a.risk_reason.clone(),
                    a.summary.clone(),
                    a.people.clone(),
                    a.dates.clone(),
                    a.amounts.clone(),
                    a.action_items.clone(),
                    true,
                    ct,
                )
            } else {
                let (cat, score) = super::classify_email(
                    &sender,
                    &subject,
                    parsed.0.as_deref(),
                    parsed.1.as_deref(),
                );
                (
                    cat,
                    score,
                    String::new(),
                    String::new(),
                    serde_json::json!([]),
                    serde_json::json!([]),
                    serde_json::json!([]),
                    serde_json::json!([]),
                    false,
                    None,
                )
            };

            // Pull invite payload / method / rsvp status directly from the
            // messages row.
            // MRS-4 (v1.5.0) writes invite_payload + invite_method on inbound
            // delivery; MRS-14 lazy-backfills NULL columns on read.
            // MRS-19 adds rsvp_status + rsvp_at, written by the RSVP API.
            let (invite_payload, invite_method, rsvp_status, rsvp_at) =
                if let Some(ref pool) = state.pg_pool {
                    let stored: (
                        Option<serde_json::Value>,
                        Option<String>,
                        Option<String>,
                        Option<chrono::DateTime<chrono::Utc>>,
                    ) = sqlx::query_as(
                        "SELECT invite_payload, invite_method, rsvp_status, rsvp_at
                         FROM messages WHERE id = $1",
                    )
                    .bind(msg.id)
                    .fetch_optional(pool)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or((None, None, None, None));

                    if stored.0.is_some() {
                        stored
                    } else if let Some(ref raw_bytes) = raw {
                        match try_lazy_backfill_invite(pool, msg.id, raw_bytes.as_slice()).await {
                            Some((p, m)) => (Some(p), Some(m), stored.2, stored.3),
                            None => stored,
                        }
                    } else {
                        stored
                    }
                } else {
                    (None, None, None, None)
                };

            return Json(Some(MessageDetail {
                uid: msg.uid,
                sender,
                recipients: msg.recipients,
                subject,
                size: msg.size,
                flags: msg.flags,
                internal_date: msg.internal_date,
                text_body: parsed.0,
                html_body: parsed.1,
                attachments: parsed.2,
                category,
                risk_score,
                risk_reason,
                summary,
                people,
                dates,
                amounts,
                action_items,
                ai_analyzed,
                clean_text,
                invite_payload,
                invite_method,
                rsvp_status,
                rsvp_at,
            }));
        }
    }

    Json(None::<MessageDetail>)
}

pub(crate) async fn update_message_flags(
    Path(uid): Path<u32>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(update): Json<FlagUpdate>,
) -> impl IntoResponse {
    if !matches!(update.action.as_str(), "add" | "remove" | "set") {
        return Json(ApiResult {
            success: false,
            message: Some("action must be one of: add, remove, set".into()),
        });
    }

    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if mb_store
            .get_message(mb.id, uid)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            let result = match update.action.as_str() {
                "add" => mb_store.add_flags(mb.id, uid, update.flags).await,
                "remove" => mb_store.remove_flags(mb.id, uid, update.flags).await,
                _ => mb_store.update_flags(mb.id, uid, update.flags).await,
            };
            if let Err(e) = &result {
                tracing::error!(event = "update_flags_failed", error = %e);
            }
            return Json(ApiResult {
                success: result.is_ok(),
                message: result.err().map(|_| "failed to update flags".into()),
            });
        }
    }

    Json(ApiResult {
        success: false,
        message: Some("message not found".into()),
    })
}

pub(crate) async fn delete_message(
    Path(uid): Path<u32>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    // mark as deleted
    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if mb_store
            .get_message(mb.id, uid)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            let result = mb_store
                .add_flags(mb.id, uid, mailrs_mailbox::FLAG_DELETED)
                .await;
            if let Err(e) = &result {
                tracing::error!(event = "delete_message_failed", error = %e);
            }
            return Json(ApiResult {
                success: result.is_ok(),
                message: result.err().map(|_| "failed to delete message".into()),
            });
        }
    }

    Json(ApiResult {
        success: false,
        message: Some("message not found".into()),
    })
}

pub(crate) async fn get_message_raw(
    Path(uid): Path<u32>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return (
            StatusCode::NOT_FOUND,
            [
                ("content-type", "text/plain".to_string()),
                ("content-disposition", String::new()),
            ],
            b"mailbox not configured".to_vec(),
        );
    };

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await
            && let Some(data) =
                message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id).await
        {
            let subject = message_util::decode_header(&msg.subject);
            let safe_name = subject
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            let filename = if safe_name.trim().is_empty() {
                format!("message-{uid}.eml")
            } else {
                format!("{}.eml", safe_name.trim())
            };
            let disposition = format!("attachment; filename=\"{filename}\"");
            return (
                StatusCode::OK,
                [
                    ("content-type", "message/rfc822".to_string()),
                    ("content-disposition", disposition),
                ],
                data,
            );
        }
    }

    (
        StatusCode::NOT_FOUND,
        [
            ("content-type", "text/plain".to_string()),
            ("content-disposition", String::new()),
        ],
        b"message not found".to_vec(),
    )
}

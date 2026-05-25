use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use super::*;
use crate::message_util;

#[derive(Deserialize)]
pub(crate) struct AuditLogQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: i64,
}

pub(crate) async fn get_audit_log(
    Query(query): Query<AuditLogQuery>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.accounts") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let limit = query.limit.clamp(1, 1000);
    let entries = ds.list_audit_log(limit).await.unwrap_or_default();
    Json(serde_json::to_value(entries).unwrap_or_default())
}

// ---------- mail audit ----------

#[derive(Deserialize)]
pub(crate) struct AuditAccountsQuery {
    #[serde(default)]
    pub domain: Option<String>,
}

/// list accounts available for audit (filtered by accessible domains)
pub(crate) async fn audit_list_accounts(
    Query(q): Query<AuditAccountsQuery>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.impersonate") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let accounts = ds.list_accounts().await.unwrap_or_default();
    let accessible = permissions.accessible_domains();
    let filtered: Vec<_> = accounts
        .into_iter()
        .filter(|a| {
            // domain scope check
            if !permissions.is_super() && !accessible.iter().any(|d| d == &a.domain) {
                return false;
            }
            // optional domain filter
            if let Some(ref d) = q.domain {
                return a.domain == *d;
            }
            true
        })
        .collect();
    Json(serde_json::to_value(filtered).unwrap_or_default())
}

#[derive(Deserialize)]
pub(crate) struct AuditConversationsQuery {
    pub target_user: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub before: Option<i64>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
}

/// list conversations for the target user (audit mode)
pub(crate) async fn audit_list_conversations(
    Query(q): Query<AuditConversationsQuery>,
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Err(e) = validate_audit_target(&q.target_user, permissions) {
        return e.into_response();
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<conversations::ConversationResponse>::new()).into_response();
    };

    let limit = clamp_limit(q.limit);
    let convos = mb_store
        .list_conversations(
            &q.target_user,
            limit,
            q.before,
            q.category.as_deref(),
            None,
            false,
            q.folder.as_deref(),
            None,
            None,
            None,
        )
        .await
        .unwrap_or_default();

    // audit log
    if let Some(ref ds) = state.domain_store {
        ds.log_audit(address, "audit.list_conversations", &q.target_user, "")
            .await;
    }

    Json(conversations::convos_to_response(convos)).into_response()
}

/// get thread messages for the target user (audit mode)
pub(crate) async fn audit_get_thread_messages(
    Path(thread_id): Path<String>,
    Query(q): Query<AuditTargetQuery>,
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Err(e) = validate_audit_target(&q.target_user, permissions) {
        return e.into_response();
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<conversations::ThreadMessageResponse>::new()).into_response();
    };

    if thread_id.len() > MAX_PATH_LEN {
        return Json(Vec::<conversations::ThreadMessageResponse>::new()).into_response();
    }

    let messages = mb_store
        .list_thread_messages(&q.target_user, &thread_id, None)
        .await
        .unwrap_or_default();

    let mut result = Vec::with_capacity(messages.len());
    for msg in &messages {
        let maildir_user = if msg.user_address.is_empty() {
            &q.target_user
        } else {
            &msg.user_address
        };
        let raw =
            message_util::read_message_raw(&state.maildir_root, maildir_user, &msg.maildir_id);
        let parsed = raw
            .as_deref()
            .map(message_util::parse_message)
            .unwrap_or_default();

        let (sender, subject) = if msg.sender.is_empty() || msg.subject.is_empty() {
            let raw_sender = raw
                .as_deref()
                .map(|d| message_util::extract_header_from_raw(d, "From"))
                .unwrap_or_default();
            let raw_subject = raw
                .as_deref()
                .map(|d| message_util::extract_header_from_raw(d, "Subject"))
                .unwrap_or_default();
            (
                if msg.sender.is_empty() {
                    message_util::decode_header(&raw_sender)
                } else {
                    message_util::decode_header(&msg.sender)
                },
                if msg.subject.is_empty() {
                    message_util::decode_header(&raw_subject)
                } else {
                    message_util::decode_header(&msg.subject)
                },
            )
        } else {
            (
                message_util::decode_header(&msg.sender),
                message_util::decode_header(&msg.subject),
            )
        };

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
            let (cat, score) =
                classify_email(&sender, &subject, parsed.0.as_deref(), parsed.1.as_deref());
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

        let structured_data = parsed.1.as_deref().and_then(|html| {
            let sd = mailrs_intelligence::structured::extract_structured_data(html);
            if sd.is_empty() { None } else { Some(sd) }
        });

        result.push(conversations::ThreadMessageResponse {
            id: msg.id,
            uid: msg.uid,
            sender,
            recipients: msg.recipients.clone(),
            subject,
            flags: msg.flags,
            internal_date: msg.internal_date,
            message_id: msg.message_id.clone(),
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
            new_content: msg.new_content.clone(),
            importance_level: msg.importance_level.clone(),
            importance_score: msg.importance_score,
            is_bulk_sender: msg.is_bulk_sender,
            has_tracking_pixel: msg.has_tracking_pixel,
            requires_action: ai.as_ref().is_some_and(|a| a.requires_action),
            sender_intent: ai
                .as_ref()
                .map_or_else(|| "inform".into(), |a| a.sender_intent.clone()),
            action_deadline: ai.as_ref().and_then(|a| a.action_deadline.clone()),
            structured_data,
            invite_method: None,
        });
    }

    // audit log
    if let Some(ref ds) = state.domain_store {
        ds.log_audit(address, "audit.read_thread", &q.target_user, &thread_id)
            .await;
    }

    Json(result).into_response()
}

#[derive(Deserialize)]
pub(crate) struct AuditTargetQuery {
    pub target_user: String,
}

/// get raw .eml for a target user's message (audit mode)
pub(crate) async fn audit_get_raw_message(
    Path(uid): Path<u32>,
    Query(q): Query<AuditTargetQuery>,
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Err(e) = validate_audit_target(&q.target_user, permissions) {
        return e.into_response();
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        })
        .into_response();
    };

    // find the message across the target user's mailboxes
    let mailboxes = mb_store
        .list_mailboxes(&q.target_user)
        .await
        .unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await {
            let maildir_user = &q.target_user;
            if let Some(data) =
                message_util::read_message_raw(&state.maildir_root, maildir_user, &msg.maildir_id)
            {
                // audit log
                if let Some(ref ds) = state.domain_store {
                    ds.log_audit(address, "audit.read_raw", &q.target_user, &uid.to_string())
                        .await;
                }
                return (
                    StatusCode::OK,
                    [
                        (axum::http::header::CONTENT_TYPE, "message/rfc822"),
                        (
                            axum::http::header::CONTENT_DISPOSITION,
                            "attachment; filename=\"message.eml\"",
                        ),
                    ],
                    data,
                )
                    .into_response();
            }
        }
    }

    Json(ApiResult {
        success: false,
        message: Some("message not found".into()),
    })
    .into_response()
}

// --- audit / eDiscovery export ---

#[derive(Deserialize)]
pub(crate) struct ExportQuery {
    pub user: String,
    #[serde(default)]
    pub from_date: Option<String>,
    #[serde(default)]
    pub to_date: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default = "default_export_limit")]
    pub limit: i64,
}

pub(crate) async fn export_messages(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Query(q): Query<ExportQuery>,
) -> impl IntoResponse {
    use axum::http::header;

    if let Some(resp) = require_permission(permissions, "admin.accounts") {
        return resp.into_response();
    }
    let Some(ref pool) = state.pg_pool else {
        return (StatusCode::SERVICE_UNAVAILABLE, "pg not available").into_response();
    };

    let mut conditions = vec!["mb.user_address = $1".to_string()];
    let mut param_idx = 2u32;

    if q.from_date.is_some() {
        conditions.push(format!(
            "m.internal_date >= EXTRACT(EPOCH FROM ${}::TIMESTAMPTZ)",
            param_idx
        ));
        param_idx += 1;
    }
    if q.to_date.is_some() {
        conditions.push(format!(
            "m.internal_date <= EXTRACT(EPOCH FROM ${}::TIMESTAMPTZ)",
            param_idx
        ));
        param_idx += 1;
    }
    if q.query.is_some() {
        conditions.push(format!(
            "(m.subject ILIKE '%' || ${} || '%' OR m.text_body ILIKE '%' || ${} || '%')",
            param_idx, param_idx
        ));
        param_idx += 1;
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT m.message_id, m.sender, m.recipients, m.subject, \
                m.internal_date, m.size, m.text_body, mb.name as folder \
         FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id \
         WHERE {where_clause} \
         ORDER BY m.internal_date DESC LIMIT ${param_idx}"
    );

    let mut query = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            Option<String>,
            i64,
            i32,
            Option<String>,
            String,
        ),
    >(&sql);
    query = query.bind(&q.user);
    if let Some(ref d) = q.from_date {
        query = query.bind(d);
    }
    if let Some(ref d) = q.to_date {
        query = query.bind(d);
    }
    if let Some(ref s) = q.query {
        query = query.bind(s);
    }
    query = query.bind(q.limit);

    let rows = match query.fetch_all(pool).await {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // export as JSON lines
    let mut output = String::new();
    for (message_id, sender, recipients, subject, date, size, body, folder) in &rows {
        let obj = serde_json::json!({
            "message_id": message_id,
            "sender": sender,
            "recipients": recipients,
            "subject": subject,
            "date": date,
            "size": size,
            "body_preview": body.as_deref().unwrap_or("").chars().take(500).collect::<String>(),
            "folder": folder,
        });
        output.push_str(&obj.to_string());
        output.push('\n');
    }

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/jsonl".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"export-{}.jsonl\"", q.user),
            ),
        ],
        output,
    )
        .into_response()
}

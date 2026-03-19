use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::message_util;

use super::{validate_domains, ApiResult, AuthUser, DomainsQuery, WebState};

#[derive(Serialize)]
pub(super) struct ConversationResponse {
    pub thread_id: String,
    pub subject: String,
    pub participants: Vec<String>,
    pub message_count: u32,
    pub unread_count: u32,
    pub last_date: i64,
    pub category: String,
    pub flagged: bool,
    pub snippet: String,
    pub pinned: bool,
    pub archived: bool,
    pub importance_level: String,
    pub importance_score: f32,
}

#[derive(Serialize)]
pub(super) struct ThreadMessageResponse {
    pub id: i64,
    pub uid: u32,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub flags: u32,
    pub internal_date: i64,
    pub message_id: String,
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
    pub new_content: Option<String>,
    pub importance_level: String,
    pub importance_score: f32,
    pub is_bulk_sender: bool,
    pub has_tracking_pixel: bool,
    pub requires_action: bool,
    pub sender_intent: String,
    pub action_deadline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_data: Option<crate::structured_data::StructuredData>,
}

#[derive(Serialize)]
pub(super) struct CategoryCount {
    pub category: String,
    pub count: i64,
}

#[derive(Serialize)]
pub(super) struct SemanticSearchResult {
    pub thread_id: String,
    pub similarity: f64,
}

#[derive(Deserialize)]
pub(super) struct ConversationsQuery {
    #[serde(default = "super::default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub before: Option<i64>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub domains: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct SearchQuery {
    pub q: String,
    #[serde(default = "super::default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub domains: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ContactsQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_contacts_limit")]
    pub limit: u32,
}

fn default_contacts_limit() -> u32 {
    20
}

pub(super) fn convos_to_response(
    convos: Vec<mailrs_mailbox::ConversationSummary>,
) -> Vec<ConversationResponse> {
    convos
        .into_iter()
        .map(|c| ConversationResponse {
            thread_id: c.thread_id,
            subject: message_util::decode_header(&c.subject),
            participants: c
                .participants
                .split(',')
                .map(|s| message_util::decode_header(s.trim()))
                .collect(),
            message_count: c.message_count,
            unread_count: c.unread_count,
            last_date: c.last_date,
            category: c.category,
            flagged: c.flagged,
            snippet: c.snippet,
            pinned: c.pinned,
            archived: c.archived,
            importance_level: c.importance_level,
            importance_score: c.importance_score,
        })
        .collect()
}

pub(super) async fn get_conversations(
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    Query(q): Query<ConversationsQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ConversationResponse>::new());
    };

    let _ = mb_store.ensure_default_mailboxes(user).await;

    let limit = super::clamp_limit(q.limit);
    let domains = validate_domains(q.domains.as_deref(), permissions);

    let convos = mb_store
        .list_conversations(
            user,
            limit,
            q.before,
            q.category.as_deref(),
            domains.as_deref(),
            q.archived,
            q.folder.as_deref(),
        )
        .await
        .unwrap_or_default();

    Json(convos_to_response(convos))
}

pub(super) async fn get_thread_messages(
    Path(thread_id): Path<String>,
    Query(dq): Query<DomainsQuery>,
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ThreadMessageResponse>::new());
    };

    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(Vec::<ThreadMessageResponse>::new());
    }

    let domains = validate_domains(dq.domains.as_deref(), permissions);

    let messages = mb_store
        .list_thread_messages(user, &thread_id, domains.as_deref())
        .await
        .unwrap_or_default();

    let mut result = Vec::with_capacity(messages.len());
    for msg in &messages {
        // in supermode, use the message owner's maildir; otherwise use current user
        let maildir_user = if msg.user_address.is_empty() {
            user
        } else {
            &msg.user_address
        };
        let raw =
            message_util::read_message_raw(&state.maildir_root, maildir_user, &msg.maildir_id);
        let parsed = raw
            .as_deref()
            .map(message_util::parse_message)
            .unwrap_or_default();

        // fallback: extract sender/subject from raw email if DB values are empty
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
            let (cat, score) =
                super::classify_email(&sender, &subject, parsed.0.as_deref(), parsed.1.as_deref());
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

        // extract structured data from HTML before moving it
        let structured_data = parsed.1.as_deref().and_then(|html| {
            let sd = crate::structured_data::extract_structured_data(html);
            if sd.is_empty() { None } else { Some(sd) }
        });

        result.push(ThreadMessageResponse {
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
            sender_intent: ai.as_ref().map_or_else(|| "inform".into(), |a| a.sender_intent.clone()),
            action_deadline: ai.as_ref().and_then(|a| a.action_deadline.clone()),
            structured_data,
        });
    }

    Json(result)
}

pub(super) async fn mark_thread_read(
    Path(thread_id): Path<String>,
    Query(dq): Query<DomainsQuery>,
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("thread id too long".into()),
        });
    }

    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let domains = validate_domains(dq.domains.as_deref(), permissions);

    match mb_store
        .mark_thread_read(user, &thread_id, domains.as_deref())
        .await
    {
        Ok(count) => Json(ApiResult {
            success: true,
            message: Some(format!("{count} messages marked as read")),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn mark_thread_unread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("thread id too long".into()),
        });
    }

    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.mark_thread_unread(&user, &thread_id).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some("thread marked as unread".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn delete_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("thread id too long".into()),
        });
    }

    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let maildir_ids = match mb_store.delete_thread(&user, &thread_id).await {
        Ok(ids) => ids,
        Err(e) => {
            return Json(ApiResult {
                success: false,
                message: Some(e.to_string()),
            });
        }
    };

    // delete physical maildir files if user address is valid
    if let Some((local, domain)) = user.split_once('@') {
        let maildir_path = format!("{}/{}/{}", state.maildir_root, domain, local);
        let md = mailrs_storage_maildir::Maildir::open(&maildir_path);
        let cur = md.scan_cur().unwrap_or_default();
        let new_entries = md.scan_new().unwrap_or_default();
        let id_set: std::collections::HashSet<String> = maildir_ids.iter().cloned().collect();
        for entry in cur.into_iter().chain(new_entries) {
            if id_set.contains(&entry.id.to_string()) {
                let _ = std::fs::remove_file(&entry.path);
            }
        }
    }

    Json(ApiResult {
        success: true,
        message: Some(format!("deleted {} messages", maildir_ids.len())),
    })
}

pub(super) async fn get_conversation_categories(
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    Query(dq): Query<DomainsQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<CategoryCount>::new());
    };

    let domains = validate_domains(dq.domains.as_deref(), permissions);

    let cats = mb_store
        .list_conversation_categories(user, domains.as_deref())
        .await
        .unwrap_or_default();

    let result: Vec<CategoryCount> = cats
        .into_iter()
        .map(|(category, count)| CategoryCount { category, count })
        .collect();

    Json(result)
}

pub(super) async fn search_conversations(
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    Query(q): Query<SearchQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ConversationResponse>::new());
    };

    if q.q.len() > super::MAX_QUERY_LEN {
        return Json(Vec::<ConversationResponse>::new());
    }

    let limit = super::clamp_limit(q.limit);
    let domains = validate_domains(q.domains.as_deref(), permissions);

    let mut convos = mb_store
        .search_conversations(
            user,
            &q.q,
            limit,
            q.category.as_deref(),
            domains.as_deref(),
        )
        .await
        .unwrap_or_default();

    // supplement with semantic search when text search returns few results
    if convos.len() < limit as usize {
        if let Some(extra) = semantic_search_threads(
            &state,
            user,
            &q.q,
            limit as usize - convos.len(),
            q.category.as_deref(),
            domains.as_deref(),
        )
        .await
        {
            let existing: std::collections::HashSet<String> =
                convos.iter().map(|c| c.thread_id.clone()).collect();
            for c in extra {
                if !existing.contains(&c.thread_id) {
                    convos.push(c);
                }
            }
        }
    }

    Json(convos_to_response(convos))
}

/// run semantic search and build ConversationSummary for each matching thread
async fn semantic_search_threads(
    state: &WebState,
    user: &str,
    query: &str,
    max: usize,
    category: Option<&str>,
    domains: Option<&[String]>,
) -> Option<Vec<mailrs_mailbox::ConversationSummary>> {
    let llm = state.llm_config.as_ref()?;
    let mb = state.mailbox_store.as_ref()?;

    let embedding = crate::ai_email::generate_embedding(llm, query).await?;
    let results = mb
        .semantic_search(user, &embedding, max.min(20) as i64, domains)
        .await
        .ok()?;

    let mut out = Vec::new();
    for (_, thread_id, _) in &results {
        let msgs = mb
            .list_thread_messages(user, thread_id, domains)
            .await
            .ok()?;
        let first = msgs.first()?;
        let last = msgs.last()?;

        let cat = mb
            .get_email_analysis(last.id)
            .await
            .ok()
            .flatten()
            .map(|a| a.category)
            .unwrap_or_else(|| "general".to_string());

        if let Some(filter) = category {
            if cat != filter {
                continue;
            }
        }

        let participants: Vec<String> = msgs
            .iter()
            .map(|m| m.sender.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        out.push(mailrs_mailbox::ConversationSummary {
            thread_id: thread_id.clone(),
            subject: first.subject.clone(),
            participants: participants.join(","),
            message_count: msgs.len() as u32,
            unread_count: msgs.iter().filter(|m| m.flags & 1 == 0).count() as u32,
            last_date: last.internal_date,
            category: cat,
            flagged: msgs.iter().any(|m| m.flags & 4 != 0),
            snippet: String::new(),
            pinned: false,
            archived: false,
            importance_level: msgs.iter().max_by(|a, b| a.importance_score.partial_cmp(&b.importance_score).unwrap_or(std::cmp::Ordering::Equal)).map(|m| m.importance_level.clone()).unwrap_or_else(|| "normal".into()),
            importance_score: msgs.iter().map(|m| m.importance_score).fold(0.0f32, f32::max),
        });
    }

    Some(out)
}

pub(super) async fn semantic_search(
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    Query(q): Query<SearchQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<SemanticSearchResult>::new());
    };
    let Some(ref llm_config) = state.llm_config else {
        return Json(Vec::<SemanticSearchResult>::new());
    };

    if q.q.len() > super::MAX_QUERY_LEN {
        return Json(Vec::<SemanticSearchResult>::new());
    }

    let limit = super::clamp_limit(q.limit);

    let embedding = match crate::ai_email::generate_embedding(llm_config, &q.q).await {
        Some(e) => e,
        None => return Json(Vec::<SemanticSearchResult>::new()),
    };

    let domains = validate_domains(q.domains.as_deref(), permissions);

    let results = mb_store
        .semantic_search(user, &embedding, limit as i64, domains.as_deref())
        .await
        .unwrap_or_default();

    let mut seen = std::collections::HashMap::new();
    for (_, thread_id, similarity) in &results {
        let entry = seen.entry(thread_id.clone()).or_insert(*similarity);
        if *similarity > *entry {
            *entry = *similarity;
        }
    }

    let result: Vec<SemanticSearchResult> = seen
        .into_iter()
        .map(|(thread_id, similarity)| SemanticSearchResult {
            thread_id,
            similarity,
        })
        .collect();

    Json(result)
}

// ---- snooze API ----

#[derive(Deserialize)]
pub(super) struct SnoozeRequest {
    pub until: String,
}

pub(super) async fn snooze_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SnoozeRequest>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let until = match req.until.parse::<chrono::DateTime<chrono::Utc>>() {
        Ok(dt) => dt,
        Err(_) => {
            return Json(ApiResult {
                success: false,
                message: Some("invalid datetime format".into()),
            });
        }
    };

    match mb_store.snooze_thread(&user, &thread_id, until).await {
        Ok(()) => Json(ApiResult {
            success: true,
            message: Some("thread snoozed".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn unsnooze_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.unsnooze_thread(&user, &thread_id).await {
        Ok(()) => Json(ApiResult {
            success: true,
            message: Some("thread unsnoozed".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

// ---- feedback API ----

#[derive(Deserialize)]
pub(super) struct FeedbackRequest {
    pub sender_email: String,
    pub action: String,
}

const VALID_FEEDBACK_ACTIONS: &[&str] = &[
    "mark_important",
    "mark_vip",
    "mark_spam",
    "block",
    "archive",
    "unblock",
];

pub(super) async fn record_feedback(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<FeedbackRequest>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    if req.sender_email.len() > 320 || !req.sender_email.contains('@') {
        return Json(ApiResult {
            success: false,
            message: Some("invalid sender email".into()),
        });
    }

    if !VALID_FEEDBACK_ACTIONS.contains(&req.action.as_str()) {
        return Json(ApiResult {
            success: false,
            message: Some(format!(
                "invalid action, must be one of: {}",
                VALID_FEEDBACK_ACTIONS.join(", ")
            )),
        });
    }

    match mb_store
        .record_sender_feedback(&user, &req.sender_email, &req.action)
        .await
    {
        Ok(()) => Json(ApiResult {
            success: true,
            message: Some(format!("feedback '{}' recorded", req.action)),
        }),
        Err(e) => {
            tracing::error!(event = "feedback_error", user = %user, error = %e);
            Json(ApiResult {
                success: false,
                message: Some("internal error".into()),
            })
        }
    }
}

pub(super) async fn get_contacts(
    AuthUser { address: user, .. }: AuthUser,
    Query(q): Query<ContactsQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<String>::new());
    };

    if q.q.len() > super::MAX_QUERY_LEN {
        return Json(Vec::<String>::new());
    }

    let limit = super::clamp_limit(q.limit);

    let contacts = mb_store
        .search_contacts(&user, &q.q, limit)
        .await
        .unwrap_or_default();

    Json(contacts)
}

// ---- mail stats for dashboard ----

#[derive(Serialize)]
pub(super) struct MailStats {
    pub total_messages: i64,
    pub unread_messages: i64,
    pub storage_bytes: u64,
    pub categories: Vec<CategoryCount>,
}

pub(super) async fn get_mail_stats(
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    Query(dq): Query<DomainsQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(MailStats {
            total_messages: 0,
            unread_messages: 0,
            storage_bytes: 0,
            categories: vec![],
        });
    };

    let domains = validate_domains(dq.domains.as_deref(), permissions);

    let total = mb_store.count_messages(user).await;
    let unread = mb_store.count_unseen(user).await;
    let storage = mb_store.user_storage_usage(user).await;

    let cats = mb_store
        .list_conversation_categories(user, domains.as_deref())
        .await
        .unwrap_or_default();
    let categories: Vec<CategoryCount> = cats
        .into_iter()
        .map(|(category, count)| CategoryCount { category, count })
        .collect();

    Json(MailStats {
        total_messages: total,
        unread_messages: unread,
        storage_bytes: storage,
        categories,
    })
}

/// batch action type
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum BatchAction {
    Read,
    Unread,
    Delete,
    Star,
    Unstar,
    Archive,
    Unarchive,
}

#[derive(Deserialize)]
pub(super) struct BatchRequest {
    pub thread_ids: Vec<String>,
    pub action: BatchAction,
}

#[derive(Serialize)]
pub(super) struct BatchResult {
    pub success: bool,
    pub processed: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub(super) async fn batch_conversations(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<BatchRequest>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(BatchResult {
            success: false,
            processed: 0,
            failed: 0,
            message: Some("mailbox not configured".into()),
        });
    };

    if req.thread_ids.is_empty() {
        return Json(BatchResult {
            success: true,
            processed: 0,
            failed: 0,
            message: Some("no thread ids provided".into()),
        });
    }

    if req.thread_ids.len() > super::MAX_BATCH_SIZE {
        return Json(BatchResult {
            success: false,
            processed: 0,
            failed: 0,
            message: Some(format!("too many thread ids (max {})", super::MAX_BATCH_SIZE)),
        });
    }

    if req.thread_ids.iter().any(|id| id.len() > super::MAX_PATH_LEN) {
        return Json(BatchResult {
            success: false,
            processed: 0,
            failed: 0,
            message: Some("thread id too long".into()),
        });
    }

    let mut processed = 0usize;
    let mut failed = 0usize;

    match req.action {
        BatchAction::Read => {
            for thread_id in &req.thread_ids {
                match mb_store.mark_thread_read(&user, thread_id, None).await {
                    Ok(_) => processed += 1,
                    Err(_) => failed += 1,
                }
            }
        }
        BatchAction::Unread => {
            for thread_id in &req.thread_ids {
                match mb_store.mark_thread_unread(&user, thread_id).await {
                    Ok(_) => processed += 1,
                    Err(_) => failed += 1,
                }
            }
        }
        BatchAction::Delete => {
            for thread_id in &req.thread_ids {
                match mb_store.delete_thread(&user, thread_id).await {
                    Ok(maildir_ids) => {
                        // delete physical maildir files
                        if let Some((local, domain)) = user.split_once('@') {
                            let maildir_path =
                                format!("{}/{}/{}", state.maildir_root, domain, local);
                            let md = mailrs_storage_maildir::Maildir::open(&maildir_path);
                            let cur = md.scan_cur().unwrap_or_default();
                            let new_entries = md.scan_new().unwrap_or_default();
                            let id_set: std::collections::HashSet<String> =
                                maildir_ids.iter().cloned().collect();
                            for entry in cur.into_iter().chain(new_entries) {
                                if id_set.contains(&entry.id.to_string()) {
                                    let _ = std::fs::remove_file(&entry.path);
                                }
                            }
                        }
                        processed += 1;
                    }
                    Err(_) => failed += 1,
                }
            }
        }
        BatchAction::Star => {
            for thread_id in &req.thread_ids {
                match mb_store.star_thread(&user, thread_id).await {
                    Ok(_) => processed += 1,
                    Err(_) => failed += 1,
                }
            }
        }
        BatchAction::Unstar => {
            for thread_id in &req.thread_ids {
                match mb_store.unstar_thread(&user, thread_id).await {
                    Ok(_) => processed += 1,
                    Err(_) => failed += 1,
                }
            }
        }
        BatchAction::Archive => {
            for thread_id in &req.thread_ids {
                match mb_store.archive_thread(&user, thread_id).await {
                    Ok(_) => processed += 1,
                    Err(_) => failed += 1,
                }
            }
        }
        BatchAction::Unarchive => {
            for thread_id in &req.thread_ids {
                match mb_store.unarchive_thread(&user, thread_id).await {
                    Ok(_) => processed += 1,
                    Err(_) => failed += 1,
                }
            }
        }
    }

    let success = failed == 0;
    let message = if failed > 0 {
        Some(format!("{processed} succeeded, {failed} failed"))
    } else {
        Some(format!("{processed} threads updated"))
    };

    Json(BatchResult {
        success,
        processed,
        failed,
        message,
    })
}

pub(super) async fn star_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.star_thread(&user, &thread_id).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some("thread starred".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn unstar_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.unstar_thread(&user, &thread_id).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some("thread unstarred".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn pin_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.pin_thread(&user, &thread_id).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some("thread pinned".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn unpin_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.unpin_thread(&user, &thread_id).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some("thread unpinned".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn dismiss_action(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.dismiss_thread_action(&user, &thread_id).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some("action dismissed".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn archive_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.archive_thread(&user, &thread_id).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some("thread archived".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn unarchive_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.unarchive_thread(&user, &thread_id).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some("thread unarchived".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

// ---- reactions API ----

#[derive(Deserialize)]
pub(super) struct ToggleReactionRequest {
    pub emoji: String,
}

#[derive(Serialize)]
pub(super) struct ReactionSummary {
    pub emoji: String,
    pub count: i64,
    pub me: bool,
}

#[derive(Serialize)]
pub(super) struct ToggleReactionResponse {
    pub reactions: Vec<ReactionSummary>,
}

#[derive(Serialize)]
pub(super) struct ThreadReactionsResponse {
    pub reactions: HashMap<i64, Vec<ReactionSummary>>,
}

pub(super) async fn toggle_reaction(
    Path((thread_id, uid)): Path<(String, i64)>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<ToggleReactionRequest>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ToggleReactionResponse { reactions: vec![] });
    }

    // validate emoji: at most 32 bytes, non-empty
    if req.emoji.is_empty() || req.emoji.len() > 32 {
        return Json(ToggleReactionResponse { reactions: vec![] });
    }

    let Some(ref pool) = state.pg_pool else {
        return Json(ToggleReactionResponse { reactions: vec![] });
    };

    // toggle: try insert, if conflict then delete
    let inserted = sqlx::query_scalar::<_, bool>(
        "INSERT INTO reactions (message_uid, thread_id, account_address, emoji)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (message_uid, account_address, emoji) DO NOTHING
         RETURNING true"
    )
    .bind(uid)
    .bind(&thread_id)
    .bind(&user)
    .bind(&req.emoji)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if inserted.is_none() {
        // row already existed — remove it
        let _ = sqlx::query(
            "DELETE FROM reactions WHERE message_uid = $1 AND account_address = $2 AND emoji = $3"
        )
        .bind(uid)
        .bind(&user)
        .bind(&req.emoji)
        .execute(pool)
        .await;
    }

    // fetch updated reactions for this message
    let reactions = fetch_message_reactions(pool, uid, &user).await;
    Json(ToggleReactionResponse { reactions })
}

pub(super) async fn get_thread_reactions(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(ThreadReactionsResponse { reactions: HashMap::new() });
    }

    let Some(ref pool) = state.pg_pool else {
        return Json(ThreadReactionsResponse { reactions: HashMap::new() });
    };

    let rows = sqlx::query_as::<_, (i64, String, i64, bool)>(
        "SELECT message_uid, emoji, COUNT(*) as cnt,
                bool_or(account_address = $1) as me
         FROM reactions
         WHERE thread_id = $2
         GROUP BY message_uid, emoji
         ORDER BY message_uid, emoji"
    )
    .bind(&user)
    .bind(&thread_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut reactions: HashMap<i64, Vec<ReactionSummary>> = HashMap::new();
    for (message_uid, emoji, count, me) in rows {
        reactions
            .entry(message_uid)
            .or_default()
            .push(ReactionSummary { emoji, count, me });
    }

    Json(ThreadReactionsResponse { reactions })
}

async fn fetch_message_reactions(
    pool: &sqlx::PgPool,
    message_uid: i64,
    current_user: &str,
) -> Vec<ReactionSummary> {
    sqlx::query_as::<_, (String, i64, bool)>(
        "SELECT emoji, COUNT(*) as cnt,
                bool_or(account_address = $1) as me
         FROM reactions
         WHERE message_uid = $2
         GROUP BY emoji
         ORDER BY emoji"
    )
    .bind(current_user)
    .bind(message_uid)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(emoji, count, me)| ReactionSummary { emoji, count, me })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- convos_to_response: response structure for agent consumption ---

    fn make_summary(thread_id: &str, subject: &str, participants: &str) -> mailrs_mailbox::ConversationSummary {
        mailrs_mailbox::ConversationSummary {
            thread_id: thread_id.to_string(),
            subject: subject.to_string(),
            participants: participants.to_string(),
            message_count: 3,
            unread_count: 1,
            last_date: 1700000000,
            category: "personal".to_string(),
            flagged: false,
            snippet: "hello world".to_string(),
            pinned: false,
            archived: false,
            importance_level: "normal".to_string(),
            importance_score: 0.5,
        }
    }

    #[test]
    fn convos_to_response_maps_all_fields() {
        let input = vec![make_summary("thread-1", "Test Subject", "alice@example.com")];
        let result = convos_to_response(input);

        assert_eq!(result.len(), 1);
        let r = &result[0];
        assert_eq!(r.thread_id, "thread-1");
        assert_eq!(r.subject, "Test Subject");
        assert_eq!(r.participants, vec!["alice@example.com"]);
        assert_eq!(r.message_count, 3);
        assert_eq!(r.unread_count, 1);
        assert_eq!(r.last_date, 1700000000);
        assert_eq!(r.category, "personal");
        assert!(!r.flagged);
        assert_eq!(r.snippet, "hello world");
        assert!(!r.pinned);
        assert!(!r.archived);
        assert_eq!(r.importance_level, "normal");
        assert!((r.importance_score - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn convos_to_response_splits_participants() {
        let input = vec![make_summary(
            "thread-2",
            "Multi",
            "alice@a.com, bob@b.com, carol@c.com",
        )];
        let result = convos_to_response(input);
        assert_eq!(
            result[0].participants,
            vec!["alice@a.com", "bob@b.com", "carol@c.com"]
        );
    }

    #[test]
    fn convos_to_response_empty_input() {
        let result = convos_to_response(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn convos_to_response_multiple_conversations() {
        let input = vec![
            make_summary("t1", "First", "a@a.com"),
            make_summary("t2", "Second", "b@b.com"),
            make_summary("t3", "Third", "c@c.com"),
        ];
        let result = convos_to_response(input);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].thread_id, "t1");
        assert_eq!(result[1].thread_id, "t2");
        assert_eq!(result[2].thread_id, "t3");
    }

    // --- conversation response JSON shape for agent consumption ---

    #[test]
    fn conversation_response_serializes_all_agent_fields() {
        let r = ConversationResponse {
            thread_id: "t1".to_string(),
            subject: "Test".to_string(),
            participants: vec!["user@example.com".to_string()],
            message_count: 5,
            unread_count: 2,
            last_date: 1700000000,
            category: "personal".to_string(),
            flagged: true,
            snippet: "preview text".to_string(),
            pinned: false,
            archived: false,
            importance_level: "high".to_string(),
            importance_score: 0.9,
        };

        let json = serde_json::to_value(&r).unwrap();

        // verify all fields agents need are present
        assert!(json.get("thread_id").is_some());
        assert!(json.get("subject").is_some());
        assert!(json.get("participants").is_some());
        assert!(json.get("message_count").is_some());
        assert!(json.get("unread_count").is_some());
        assert!(json.get("last_date").is_some());
        assert!(json.get("category").is_some());
        assert!(json.get("flagged").is_some());
        assert!(json.get("snippet").is_some());
        assert!(json.get("pinned").is_some());
        assert!(json.get("archived").is_some());
        assert!(json.get("importance_level").is_some());
        assert!(json.get("importance_score").is_some());

        // verify types
        assert!(json["participants"].is_array());
        assert!(json["message_count"].is_number());
        assert!(json["last_date"].is_number());
        assert!(json["flagged"].is_boolean());
    }

    // --- thread message response JSON shape ---

    #[test]
    fn thread_message_response_serializes_body_fields() {
        let r = ThreadMessageResponse {
            id: 1,
            uid: 100,
            sender: "alice@example.com".to_string(),
            recipients: "bob@example.com".to_string(),
            subject: "Test".to_string(),
            flags: 0,
            internal_date: 1700000000,
            message_id: "<msg1@example.com>".to_string(),
            text_body: Some("plain text content".to_string()),
            html_body: Some("<p>html content</p>".to_string()),
            attachments: vec![],
            category: "personal".to_string(),
            risk_score: 0,
            risk_reason: String::new(),
            summary: String::new(),
            people: serde_json::json!([]),
            dates: serde_json::json!([]),
            amounts: serde_json::json!([]),
            action_items: serde_json::json!([]),
            ai_analyzed: false,
            clean_text: None,
            new_content: None,
            importance_level: "normal".to_string(),
            importance_score: 0.5,
            is_bulk_sender: false,
            has_tracking_pixel: false,
            requires_action: false,
            sender_intent: "inform".to_string(),
            action_deadline: None,
            structured_data: None,
        };

        let json = serde_json::to_value(&r).unwrap();

        // critical fields for agent read operations
        assert!(json.get("text_body").is_some());
        assert!(json.get("html_body").is_some());
        assert!(json.get("attachments").is_some());
        assert!(json.get("sender").is_some());
        assert!(json.get("recipients").is_some());
        assert!(json.get("subject").is_some());
        assert!(json.get("message_id").is_some());

        // verify body content is accessible
        assert_eq!(json["text_body"].as_str().unwrap(), "plain text content");
        assert_eq!(json["html_body"].as_str().unwrap(), "<p>html content</p>");

        // agent intelligence fields
        assert!(json.get("category").is_some());
        assert!(json.get("risk_score").is_some());
        assert!(json.get("summary").is_some());
        assert!(json.get("importance_level").is_some());
        assert!(json.get("requires_action").is_some());
        assert!(json.get("sender_intent").is_some());
    }

    #[test]
    fn thread_message_response_omits_structured_data_when_none() {
        let r = ThreadMessageResponse {
            id: 1,
            uid: 100,
            sender: String::new(),
            recipients: String::new(),
            subject: String::new(),
            flags: 0,
            internal_date: 0,
            message_id: String::new(),
            text_body: None,
            html_body: None,
            attachments: vec![],
            category: "general".to_string(),
            risk_score: 0,
            risk_reason: String::new(),
            summary: String::new(),
            people: serde_json::json!([]),
            dates: serde_json::json!([]),
            amounts: serde_json::json!([]),
            action_items: serde_json::json!([]),
            ai_analyzed: false,
            clean_text: None,
            new_content: None,
            importance_level: "normal".to_string(),
            importance_score: 0.0,
            is_bulk_sender: false,
            has_tracking_pixel: false,
            requires_action: false,
            sender_intent: "inform".to_string(),
            action_deadline: None,
            structured_data: None,
        };

        let json = serde_json::to_value(&r).unwrap();
        // structured_data has skip_serializing_if = "Option::is_none"
        assert!(json.get("structured_data").is_none());
    }

    // --- query parameter deserialization ---

    #[test]
    fn conversations_query_defaults() {
        let q: ConversationsQuery =
            serde_json::from_str("{}").unwrap();
        assert_eq!(q.limit, 50); // default_limit()
        assert!(q.before.is_none());
        assert!(q.category.is_none());
        assert!(q.domains.is_none());
        assert!(!q.archived);
        assert!(q.folder.is_none());
    }

    #[test]
    fn conversations_query_with_params() {
        let q: ConversationsQuery = serde_json::from_str(
            r#"{"limit":10,"before":1700000000,"category":"personal","folder":"INBOX","archived":true}"#
        ).unwrap();
        assert_eq!(q.limit, 10);
        assert_eq!(q.before, Some(1700000000));
        assert_eq!(q.category.as_deref(), Some("personal"));
        assert_eq!(q.folder.as_deref(), Some("INBOX"));
        assert!(q.archived);
    }

    #[test]
    fn search_query_requires_q() {
        let result: Result<SearchQuery, _> = serde_json::from_str("{}");
        assert!(result.is_err(), "search query should require 'q' field");
    }

    #[test]
    fn search_query_with_defaults() {
        let q: SearchQuery =
            serde_json::from_str(r#"{"q":"invoice"}"#).unwrap();
        assert_eq!(q.q, "invoice");
        assert_eq!(q.limit, 50);
        assert!(q.category.is_none());
        assert!(q.domains.is_none());
    }

    #[test]
    fn search_query_with_all_params() {
        let q: SearchQuery = serde_json::from_str(
            r#"{"q":"payment","limit":5,"category":"personal","domains":"example.com"}"#
        ).unwrap();
        assert_eq!(q.q, "payment");
        assert_eq!(q.limit, 5);
        assert_eq!(q.category.as_deref(), Some("personal"));
        assert_eq!(q.domains.as_deref(), Some("example.com"));
    }

    // --- superadmin domain access via API key (validates phase 1 integration) ---

    #[test]
    fn superadmin_api_key_grants_domain_access() {
        use crate::api_key_store::{self, CachedApiKey};
        use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo};

        let (full_key, _prefix, key_hash) = api_key_store::generate_api_key();
        let cached = CachedApiKey {
            key_hash,
            account_address: "admin@golia.jp".to_string(),
            expires_at: None,
            id: 1,
            app_id: None,
        };

        // verify key hash matches
        let token_hash = api_key_store::sha256_hex(full_key.as_bytes());
        assert_eq!(token_hash, cached.key_hash);

        // simulate super user permissions
        let groups = vec![AccountGroup {
            group: GroupInfo {
                id: 1,
                name: "super".into(),
                domain: None,
                description: String::new(),
                is_builtin: true,
                created_at: 0,
            },
            permissions: crate::permission::ALL_PERMISSIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }];
        let perms = compute_effective_permissions(
            &groups,
            &[],
            &["golia.jp".into(), "example.com".into()],
        );

        let result = super::super::validate_domains(Some("golia.jp,example.com"), &perms);
        assert_eq!(
            result,
            Some(vec!["golia.jp".to_string(), "example.com".to_string()])
        );
    }

    #[test]
    fn non_superadmin_cannot_access_other_domains() {
        let perms = crate::permission::compute_effective_permissions(&[], &[], &[]);
        let result = super::super::validate_domains(Some("golia.jp"), &perms);
        assert!(result.is_none());
    }

    // --- category count serialization ---

    #[test]
    fn category_count_serializes_correctly() {
        let cc = CategoryCount {
            category: "personal".to_string(),
            count: 42,
        };
        let json = serde_json::to_value(&cc).unwrap();
        assert_eq!(json["category"], "personal");
        assert_eq!(json["count"], 42);
    }
}

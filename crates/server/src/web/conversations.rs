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
            importance_level: String::from("normal"),
            importance_score: 0.0,
        })
        .collect()
}

pub(super) async fn get_conversations(
    AuthUser(user): AuthUser,
    Query(q): Query<ConversationsQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ConversationResponse>::new());
    };

    let _ = mb_store.ensure_default_mailboxes(&user).await;

    let limit = super::clamp_limit(q.limit);
    let domains = validate_domains(q.domains.as_deref(), &user, &state);

    let convos = mb_store
        .list_conversations(
            &user,
            limit,
            q.before,
            q.category.as_deref(),
            domains.as_deref(),
            q.archived,
        )
        .await
        .unwrap_or_default();

    Json(convos_to_response(convos))
}

pub(super) async fn get_thread_messages(
    Path(thread_id): Path<String>,
    Query(dq): Query<DomainsQuery>,
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ThreadMessageResponse>::new());
    };

    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(Vec::<ThreadMessageResponse>::new());
    }

    let domains = validate_domains(dq.domains.as_deref(), &user, &state);

    let messages = mb_store
        .list_thread_messages(&user, &thread_id, domains.as_deref())
        .await
        .unwrap_or_default();

    let mut result = Vec::with_capacity(messages.len());
    for msg in &messages {
        // in supermode, use the message owner's maildir; otherwise use current user
        let maildir_user = if msg.user_address.is_empty() {
            &user
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
            new_content: None,
            importance_level: String::from("normal"),
            importance_score: 0.0,
            is_bulk_sender: false,
            has_tracking_pixel: false,
        });
    }

    Json(result)
}

pub(super) async fn mark_thread_read(
    Path(thread_id): Path<String>,
    Query(dq): Query<DomainsQuery>,
    AuthUser(user): AuthUser,
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

    let domains = validate_domains(dq.domains.as_deref(), &user, &state);

    match mb_store
        .mark_thread_read(&user, &thread_id, domains.as_deref())
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
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
    Query(dq): Query<DomainsQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<CategoryCount>::new());
    };

    let domains = validate_domains(dq.domains.as_deref(), &user, &state);

    let cats = mb_store
        .list_conversation_categories(&user, domains.as_deref())
        .await
        .unwrap_or_default();

    let result: Vec<CategoryCount> = cats
        .into_iter()
        .map(|(category, count)| CategoryCount { category, count })
        .collect();

    Json(result)
}

pub(super) async fn search_conversations(
    AuthUser(user): AuthUser,
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
    let domains = validate_domains(q.domains.as_deref(), &user, &state);

    let mut convos = mb_store
        .search_conversations(
            &user,
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
            &user,
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
    let gemini = state.gemini_config.as_ref()?;
    let mb = state.mailbox_store.as_ref()?;

    let embedding = crate::ai_email::generate_embedding(gemini, query).await?;
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
        // safe: first() returned Some so vec is non-empty
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
        });
    }

    Some(out)
}

pub(super) async fn semantic_search(
    AuthUser(user): AuthUser,
    Query(q): Query<SearchQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<SemanticSearchResult>::new());
    };
    let Some(ref gemini_config) = state.gemini_config else {
        return Json(Vec::<SemanticSearchResult>::new());
    };

    if q.q.len() > super::MAX_QUERY_LEN {
        return Json(Vec::<SemanticSearchResult>::new());
    }

    let limit = super::clamp_limit(q.limit);

    // generate embedding for the query
    let embedding = match crate::ai_email::generate_embedding(gemini_config, &q.q).await {
        Some(e) => e,
        None => return Json(Vec::<SemanticSearchResult>::new()),
    };

    let domains = validate_domains(q.domains.as_deref(), &user, &state);

    let results = mb_store
        .semantic_search(&user, &embedding, limit as i64, domains.as_deref())
        .await
        .unwrap_or_default();

    // deduplicate by thread_id, keep highest similarity
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

pub(super) async fn get_contacts(
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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

pub(super) async fn archive_thread(
    Path(thread_id): Path<String>,
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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

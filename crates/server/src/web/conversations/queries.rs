//! Read-only conversations API handlers.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::conversation_cache;
use crate::message_util;

use super::super::{validate_domains, AuthUser, DomainsQuery, WebState};
use super::*;

pub(crate) async fn get_conversations(
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    Query(q): Query<ConversationsQuery>,
    State(state): State<Arc<WebState>>,
) -> Response {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ConversationResponse>::new()).into_response();
    };

    let _ = mb_store.ensure_default_mailboxes(user).await;

    let limit = crate::web::clamp_limit(q.limit);
    let domains = validate_domains(q.domains.as_deref(), permissions);

    // Try the cache first. Pagination requests (before is Some) and
    // search-driven views are cacheable too — the key encodes everything
    // that affects the response.
    let cache_key = conversation_cache::list_key(
        user,
        limit,
        q.before,
        q.category.as_deref(),
        domains.as_deref(),
        Some(q.archived),
        q.folder.as_deref(),
        q.unread,
        q.starred,
        q.section.as_deref(),
    );
    if let Some(ref valkey) = state.valkey
        && let Some(cached) = conversation_cache::get_json(valkey, &cache_key).await {
            return cached_json_response(cached);
        }

    let convos = mb_store
        .list_conversations(
            user,
            limit,
            q.before,
            q.category.as_deref(),
            domains.as_deref(),
            q.archived,
            q.folder.as_deref(),
            q.unread,
            q.starred,
            q.section.as_deref(),
        )
        .await
        .unwrap_or_default();

    let response = convos_to_response(convos);
    if let Some(ref valkey) = state.valkey
        && let Ok(json) = serde_json::to_string(&response) {
            conversation_cache::set_json(valkey, &cache_key, &json, conversation_cache::TTL_LIST_SECS).await;
        }
    Json(response).into_response()
}

pub(crate) async fn get_thread_messages(
    Path(thread_id): Path<String>,
    Query(dq): Query<DomainsQuery>,
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> Response {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ThreadMessageResponse>::new()).into_response();
    };

    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(Vec::<ThreadMessageResponse>::new()).into_response();
    }

    let domains = validate_domains(dq.domains.as_deref(), permissions);

    // Cache lookup before doing the message + maildir parse work (which can
    // be heavy for big HTML threads with attachments).
    let cache_key = conversation_cache::thread_key(user, &thread_id);
    if let Some(ref valkey) = state.valkey
        && let Some(cached) = conversation_cache::get_json(valkey, &cache_key).await {
            return cached_json_response(cached);
        }

    let messages = mb_store
        .list_thread_messages(user, &thread_id, domains.as_deref())
        .await
        .unwrap_or_default();

    // MRS-18: batch-fetch invite_method for every message in the thread so
    // the web client can mount the invite-card based on a server-
    // authoritative signal. Avoids the brittle attachments-based detection
    // that broke when conversation-API attachments parse went stale.
    let invite_methods: std::collections::HashMap<i64, String> = {
        let ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
        mb_store
            .get_invite_methods(&ids)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect()
    };

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
                crate::web::classify_email(&sender, &subject, parsed.0.as_deref(), parsed.1.as_deref());
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
            let sd = mailrs_intelligence::structured::extract_structured_data(html);
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
            invite_method: invite_methods.get(&msg.id).cloned(),
        });
    }

    if let Some(ref valkey) = state.valkey
        && let Ok(json) = serde_json::to_string(&result) {
            conversation_cache::set_json(valkey, &cache_key, &json, conversation_cache::TTL_THREAD_SECS).await;
        }
    Json(result).into_response()
}

pub(crate) async fn get_conversation_categories(
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    Query(dq): Query<DomainsQuery>,
    State(state): State<Arc<WebState>>,
) -> Response {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<CategoryCount>::new()).into_response();
    };

    let domains = validate_domains(dq.domains.as_deref(), permissions);
    let cache_key = conversation_cache::categories_key(user, domains.as_deref());
    if let Some(ref valkey) = state.valkey
        && let Some(cached) = conversation_cache::get_json(valkey, &cache_key).await {
            return cached_json_response(cached);
        }

    let cats = mb_store
        .list_conversation_categories(user, domains.as_deref())
        .await
        .unwrap_or_default();

    let result: Vec<CategoryCount> = cats
        .into_iter()
        .map(|(category, count)| CategoryCount { category, count })
        .collect();

    if let Some(ref valkey) = state.valkey
        && let Ok(json) = serde_json::to_string(&result) {
            conversation_cache::set_json(valkey, &cache_key, &json, conversation_cache::TTL_CATS_SECS).await;
        }
    Json(result).into_response()
}

pub(crate) async fn get_action_count(
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    Query(dq): Query<DomainsQuery>,
    State(state): State<Arc<WebState>>,
) -> Response {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(serde_json::json!({"count": 0})).into_response();
    };

    let domains = validate_domains(dq.domains.as_deref(), permissions);
    let cache_key = conversation_cache::action_count_key(user, domains.as_deref());
    if let Some(ref valkey) = state.valkey
        && let Some(cached) = conversation_cache::get_json(valkey, &cache_key).await {
            return cached_json_response(cached);
        }

    let count = mb_store
        .count_action_threads(user, domains.as_deref())
        .await
        .unwrap_or(0);

    let body = serde_json::json!({"count": count});
    if let Some(ref valkey) = state.valkey
        && let Ok(json) = serde_json::to_string(&body) {
            conversation_cache::set_json(valkey, &cache_key, &json, conversation_cache::TTL_ACTION_SECS).await;
        }
    Json(body).into_response()
}

pub(crate) async fn get_contacts(
    AuthUser { address: user, .. }: AuthUser,
    Query(q): Query<ContactsQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<String>::new());
    };

    if q.q.len() > crate::web::MAX_QUERY_LEN {
        return Json(Vec::<String>::new());
    }

    let limit = crate::web::clamp_limit(q.limit);

    let contacts = mb_store
        .search_contacts(&user, &q.q, limit)
        .await
        .unwrap_or_default();

    Json(contacts)
}

pub(crate) async fn get_mail_stats(
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

    // cache only the simple single-user case; cross-domain views are rare
    // enough not to warrant per-key cache invalidation work
    let cache_key = if domains.is_none() {
        Some(format!("mail:stats:v1:{user}"))
    } else {
        None
    };

    if let (Some(key), Some(valkey)) = (&cache_key, &state.valkey) {
        use redis::AsyncCommands;
        if let Ok(json) = valkey.clone().get::<_, String>(key.as_str()).await
            && let Ok(parsed) = serde_json::from_str::<MailStats>(&json) {
                return Json(parsed);
            }
    }

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

    let stats = MailStats {
        total_messages: total,
        unread_messages: unread,
        storage_bytes: storage,
        categories,
    };

    if let (Some(key), Some(valkey)) = (&cache_key, &state.valkey)
        && let Ok(json) = serde_json::to_string(&stats) {
            use redis::AsyncCommands;
            let _: redis::RedisResult<()> = valkey
                .clone()
                .set_ex(key.as_str(), json, MAIL_STATS_TTL_SECS)
                .await;
        }

    Json(stats)
}

pub(crate) async fn batch_conversations(
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

    if req.thread_ids.len() > crate::web::MAX_BATCH_SIZE {
        return Json(BatchResult {
            success: false,
            processed: 0,
            failed: 0,
            message: Some(format!("too many thread ids (max {})", crate::web::MAX_BATCH_SIZE)),
        });
    }

    if req.thread_ids.iter().any(|id| id.len() > crate::web::MAX_PATH_LEN) {
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
                            let md = mailrs_maildir::Maildir::open(&maildir_path);
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

    // batch ops can touch many threads at once — bust the whole user's
    // cache rather than per-thread (which would SCAN/DEL many times).
    if let Some(ref valkey) = state.valkey {
        conversation_cache::bust_user(valkey, &user).await;
        for tid in &req.thread_ids {
            let mut conn = valkey.clone();
            let _: Result<(), _> = redis::AsyncCommands::del::<_, ()>(
                &mut conn,
                conversation_cache::thread_key(&user, tid),
            )
            .await;
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

pub(crate) async fn get_thread_reactions(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
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

pub(super) async fn fetch_message_reactions(
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

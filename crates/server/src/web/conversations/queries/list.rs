//! List + batch fetch of conversations.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};

use crate::conversation_cache;

use super::super::super::{AuthUser, WebState, validate_domains};
use super::super::*;

pub(crate) async fn get_conversations(
    AuthUser {
        address: ref user,
        ref permissions,
        ..
    }: AuthUser,
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
        && let Some(cached) = conversation_cache::get_json(valkey, &cache_key).await
    {
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
        && let Ok(json) = serde_json::to_string(&response)
    {
        conversation_cache::set_json(valkey, &cache_key, &json, conversation_cache::TTL_LIST_SECS)
            .await;
    }
    Json(response).into_response()
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
            message: Some(format!(
                "too many thread ids (max {})",
                crate::web::MAX_BATCH_SIZE
            )),
        });
    }

    if req
        .thread_ids
        .iter()
        .any(|id| id.len() > crate::web::MAX_PATH_LEN)
    {
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

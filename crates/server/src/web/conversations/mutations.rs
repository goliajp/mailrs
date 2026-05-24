//! Mutation handlers for the conversations API.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;

use crate::conversation_cache;

use super::super::{validate_domains, ApiResult, AuthUser, DomainsQuery, WebState};
use super::*;

pub(crate) async fn mark_thread_read(
    Path(thread_id): Path<String>,
    Query(dq): Query<DomainsQuery>,
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
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

    let result = mb_store
        .mark_thread_read(user, &thread_id, domains.as_deref())
        .await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn mark_thread_unread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
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

    let result = mb_store.mark_thread_unread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, &user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn delete_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
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
        let md = mailrs_maildir::Maildir::open(&maildir_path);
        let cur = md.scan_cur().unwrap_or_default();
        let new_entries = md.scan_new().unwrap_or_default();
        let id_set: std::collections::HashSet<String> = maildir_ids.iter().cloned().collect();
        for entry in cur.into_iter().chain(new_entries) {
            if id_set.contains(&entry.id.to_string()) {
                let _ = std::fs::remove_file(&entry.path);
            }
        }
    }

    // bust caches: delete is high-impact, drop everything user-scoped
    if let Some(ref valkey) = state.valkey {
        conversation_cache::bust_thread(valkey, &user, &thread_id).await;
    }

    Json(ApiResult {
        success: true,
        message: Some(format!("deleted {} messages", maildir_ids.len())),
    })
}

pub(crate) async fn star_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let result = mb_store.star_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, &user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn unstar_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let result = mb_store.unstar_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, &user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn pin_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let result = mb_store.pin_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, &user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn unpin_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let result = mb_store.unpin_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, &user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn dismiss_action(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
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

pub(crate) async fn archive_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let result = mb_store.archive_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, &user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn unarchive_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let result = mb_store.unarchive_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, &user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn snooze_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SnoozeRequest>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
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

    let result = mb_store.snooze_thread(&user, &thread_id, until).await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, &user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn unsnooze_thread(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let result = mb_store.unsnooze_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref valkey) = state.valkey {
            conversation_cache::bust_thread(valkey, &user, &thread_id).await;
        }
    match result {
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

pub(crate) async fn record_feedback(
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

pub(crate) async fn toggle_reaction(
    Path((thread_id, uid)): Path<(String, i64)>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<ToggleReactionRequest>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
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
    let reactions = super::queries::fetch_message_reactions(pool, uid, &user).await;
    Json(ToggleReactionResponse { reactions })
}

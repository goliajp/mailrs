//! Star + pin flag toggles.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;

use crate::conversation_cache;

use super::super::super::{ApiResult, AuthUser, WebState};

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


//! Read state + archive/snooze/delete mutations.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;

use crate::conversation_cache;

use super::super::super::{ApiResult, AuthUser, DomainsQuery, WebState, validate_domains};
use super::super::*;

pub(crate) async fn mark_thread_read(
    Path(thread_id): Path<String>,
    Query(dq): Query<DomainsQuery>,
    AuthUser {
        address: ref user,
        ref permissions,
        ..
    }: AuthUser,
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
        && let Some(ref kevy) = state.kevy_embed
    {
        conversation_cache::bust_thread(kevy, user, &thread_id);
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
        && let Some(ref kevy) = state.kevy_embed
    {
        conversation_cache::bust_thread(kevy, &user, &thread_id);
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
    if let Some(ref kevy) = state.kevy_embed {
        conversation_cache::bust_thread(kevy, &user, &thread_id);
    }

    Json(ApiResult {
        success: true,
        message: Some(format!("deleted {} messages", maildir_ids.len())),
    })
}

pub(crate) async fn archive_thread(
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

    let result = mb_store.archive_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref kevy) = state.kevy_embed
    {
        conversation_cache::bust_thread(kevy, &user, &thread_id);
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

/// v2.9 triage — shared body for the five bucket-move actions. Stamps
/// the thread's `email_analysis.category` (the monolith derives the
/// triage bucket from category — see `list_conversations` folder
/// filter), so this is the spg-lane analog of fastcore's `set_bucket`.
/// Kept behaviorally identical across lanes per the core-mode parity
/// rule.
async fn set_bucket_action(
    state: &Arc<WebState>,
    user: &str,
    thread_id: &str,
    category: &str,
    ok_msg: &str,
) -> Json<ApiResult> {
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
    let result = mb_store.set_thread_bucket(user, thread_id, category).await;
    if result.is_ok() {
        if let Some(ref kevy) = state.kevy_embed {
            conversation_cache::bust_thread(kevy, user, thread_id);
            conversation_cache::bust_user(kevy, user);
        }
        // v2.9 triage — teach the SHARED classifier corpus from this
        // correction (core-mode parity with fastcore's train_triage).
        // Only the triage classes train; junk uses a separate corpus.
        if mailrs_triage::is_class(category) {
            train_triage_from_maildir(state, mb_store, user, thread_id, category).await;
        }
    }
    match result {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some(ok_msg.to_string()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

/// Read the thread's raw maildir messages, tokenize their union, and
/// train the shared `mailrs-triage` corpus (network kevy) on `class`.
/// Best-effort — no maildir file / no MAILRS_KEVY_URL / kevy hiccup all
/// silently skip. Mirrors fastcore's train_triage (maildir source +
/// shared corpus) so both lanes teach the same classifier.
async fn train_triage_from_maildir(
    state: &Arc<WebState>,
    mb_store: &mailrs_mailbox::PgMailboxStore,
    user: &str,
    thread_id: &str,
    class: &str,
) {
    let Ok(Some(url)) = std::env::var("MAILRS_KEVY_URL").map(|s| (!s.is_empty()).then_some(s))
    else {
        return;
    };
    let Ok(ids) = mb_store.get_thread_maildir_ids(user, thread_id).await else {
        return;
    };
    if ids.is_empty() {
        return;
    }
    let Some((local, domain)) = user.split_once('@') else {
        return;
    };
    let base = format!("{}/{}/{}", state.maildir_root, domain, local);
    let id_set: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut tokens: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let md = mailrs_maildir::Maildir::open(&base);
    for entry in md
        .scan_cur()
        .unwrap_or_default()
        .into_iter()
        .chain(md.scan_new().unwrap_or_default())
    {
        if id_set.contains(entry.id.to_string().as_str())
            && let Ok(raw) = std::fs::read(&entry.path)
        {
            tokens.extend(mailrs_bayes::tokenize(&raw));
        }
    }
    if tokens.is_empty() {
        return;
    }
    let Ok(mut conn) = kevy_client::Connection::open(&url) else {
        return;
    };
    let toks: Vec<String> = tokens.into_iter().collect();
    mailrs_triage::train(&mut conn, &toks, class, thread_id);
}

pub(crate) async fn mark_junk(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    set_bucket_action(&state, &user, &thread_id, "spam", "moved to Junk").await
}

pub(crate) async fn mark_not_junk(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    set_bucket_action(&state, &user, &thread_id, "inbox", "marked as not junk").await
}

pub(crate) async fn mark_notification(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    set_bucket_action(
        &state,
        &user,
        &thread_id,
        "notification",
        "moved to Notifications",
    )
    .await
}

pub(crate) async fn mark_promotion(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    set_bucket_action(
        &state,
        &user,
        &thread_id,
        "promotion",
        "moved to Promotions",
    )
    .await
}

pub(crate) async fn move_to_inbox(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    set_bucket_action(&state, &user, &thread_id, "inbox", "moved to Inbox").await
}

pub(crate) async fn unarchive_thread(
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

    let result = mb_store.unarchive_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref kevy) = state.kevy_embed
    {
        conversation_cache::bust_thread(kevy, &user, &thread_id);
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
        && let Some(ref kevy) = state.kevy_embed
    {
        conversation_cache::bust_thread(kevy, &user, &thread_id);
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

    let result = mb_store.unsnooze_thread(&user, &thread_id).await;
    if result.is_ok()
        && let Some(ref kevy) = state.kevy_embed
    {
        conversation_cache::bust_thread(kevy, &user, &thread_id);
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

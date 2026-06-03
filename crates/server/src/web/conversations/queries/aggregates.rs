//! Aggregate / counter endpoints: category counts, action count, mail stats, contacts list.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};

use crate::conversation_cache;

use super::super::super::{AuthUser, DomainsQuery, WebState, validate_domains};
use super::super::*;

pub(crate) async fn get_conversation_categories(
    AuthUser {
        address: ref user,
        ref permissions,
        ..
    }: AuthUser,
    Query(dq): Query<DomainsQuery>,
    State(state): State<Arc<WebState>>,
) -> Response {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<CategoryCount>::new()).into_response();
    };

    let domains = validate_domains(dq.domains.as_deref(), permissions);
    let cache_key = conversation_cache::categories_key(user, domains.as_deref());
    if let Some(ref kevy) = state.kevy
        && let Some(cached) = conversation_cache::get_json(kevy, &cache_key).await
    {
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

    if let Some(ref kevy) = state.kevy
        && let Ok(json) = serde_json::to_string(&result)
    {
        conversation_cache::set_json(kevy, &cache_key, &json, conversation_cache::TTL_CATS_SECS)
            .await;
    }
    Json(result).into_response()
}

pub(crate) async fn get_action_count(
    AuthUser {
        address: ref user,
        ref permissions,
        ..
    }: AuthUser,
    Query(dq): Query<DomainsQuery>,
    State(state): State<Arc<WebState>>,
) -> Response {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(serde_json::json!({"count": 0})).into_response();
    };

    let domains = validate_domains(dq.domains.as_deref(), permissions);
    let cache_key = conversation_cache::action_count_key(user, domains.as_deref());
    if let Some(ref kevy) = state.kevy
        && let Some(cached) = conversation_cache::get_json(kevy, &cache_key).await
    {
        return cached_json_response(cached);
    }

    let count = mb_store
        .count_action_threads(user, domains.as_deref())
        .await
        .unwrap_or(0);

    let body = serde_json::json!({"count": count});
    if let Some(ref kevy) = state.kevy
        && let Ok(json) = serde_json::to_string(&body)
    {
        conversation_cache::set_json(
            kevy,
            &cache_key,
            &json,
            conversation_cache::TTL_ACTION_SECS,
        )
        .await;
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
    AuthUser {
        address: ref user,
        ref permissions,
        ..
    }: AuthUser,
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

    if let (Some(key), Some(kevy)) = (&cache_key, &state.kevy) {
        use redis::AsyncCommands;
        if let Ok(json) = kevy.clone().get::<_, String>(key.as_str()).await
            && let Ok(parsed) = serde_json::from_str::<MailStats>(&json)
        {
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

    if let (Some(key), Some(kevy)) = (&cache_key, &state.kevy)
        && let Ok(json) = serde_json::to_string(&stats)
    {
        use redis::AsyncCommands;
        let _: redis::RedisResult<()> = kevy
            .clone()
            .set_ex(key.as_str(), json, MAIL_STATS_TTL_SECS)
            .await;
    }

    Json(stats)
}

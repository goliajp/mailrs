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
    if let Some(ref kevy) = state.kevy_embed
        && let Some(cached) = conversation_cache::get_json(kevy, &cache_key)
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

    if let Some(ref kevy) = state.kevy_embed
        && let Ok(json) = serde_json::to_string(&result)
    {
        conversation_cache::set_json(kevy, &cache_key, &json, conversation_cache::TTL_CATS_SECS);
    }
    Json(result).into_response()
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

    if let (Some(key), Some(store)) = (&cache_key, &state.kevy_embed)
        && let Ok(Some(bytes)) = store.get(key.as_bytes())
        && let Ok(json) = String::from_utf8(bytes)
        && let Ok(parsed) = serde_json::from_str::<MailStats>(&json)
    {
        return Json(parsed);
    }

    let total = mb_store.count_messages(user).await;
    // 0 is the display fallback, but a query error is logged loudly here
    // (the cement layer) rather than silently swallowed in the stone —
    // an unlogged swallow hid a FILTER parse failure for weeks
    let unread = mb_store.count_unseen(user).await.unwrap_or_else(|e| {
        tracing::error!(error = %e, user, "count_unseen failed — unread badge shows 0");
        0
    });
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

    if let (Some(key), Some(store)) = (&cache_key, &state.kevy_embed)
        && let Ok(json) = serde_json::to_string(&stats)
    {
        let _ = store.set_with_ttl(
            key.as_bytes(),
            json.as_bytes(),
            std::time::Duration::from_secs(MAIL_STATS_TTL_SECS),
        );
    }

    Json(stats)
}

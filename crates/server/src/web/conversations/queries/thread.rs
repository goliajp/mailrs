//! Thread (per-conversation) message fetch + reaction summaries.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};

use crate::conversation_cache;
use crate::message_util;

use super::super::super::{AuthUser, DomainsQuery, WebState, validate_domains};
use super::super::*;

pub(crate) async fn get_thread_messages(
    Path(thread_id): Path<String>,
    Query(dq): Query<DomainsQuery>,
    AuthUser {
        address: ref user,
        ref permissions,
        ..
    }: AuthUser,
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
    if let Some(ref kevy) = state.kevy_embed
        && let Some(cached) = conversation_cache::get_json(kevy, &cache_key)
    {
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

    // Per-message work is independent (maildir read, MIME parse, AI lookup)
    // — run them concurrently and preserve input order via join_all. For a
    // 25-message thread the cold-load drops from N× single-message latency
    // to roughly 1× plus scheduling overhead.
    //
    // bind shared `&T` references locally so the per-message `async move`
    // closures copy the references (refs are Copy) rather than try to move
    // out of the outer `state` / `mb_store` / `user` bindings.
    let maildir_root = &state.maildir_root;
    let mb_store_ref = mb_store.as_ref();
    let user_ref: &str = user;
    let invite_methods_ref = &invite_methods;
    let result: Vec<ThreadMessageResponse> =
        futures_util::future::join_all(messages.iter().map(|msg| async move {
            // in supermode, use the message owner's maildir; otherwise use current user
            let maildir_user = if msg.user_address.is_empty() {
                user_ref
            } else {
                msg.user_address.as_str()
            };
            let raw =
                message_util::read_message_raw(maildir_root, maildir_user, &msg.maildir_id).await;
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
            let ai = mb_store_ref.get_email_analysis(msg.id).await.ok().flatten();
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
                let (cat, score) = crate::web::classify_email(
                    &sender,
                    &subject,
                    parsed.0.as_deref(),
                    parsed.1.as_deref(),
                );
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

            ThreadMessageResponse {
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
                invite_method: invite_methods_ref.get(&msg.id).cloned(),
            }
        }))
        .await;

    if let Some(ref kevy) = state.kevy_embed
        && let Ok(json) = serde_json::to_string(&result)
    {
        conversation_cache::set_json(kevy, &cache_key, &json, conversation_cache::TTL_THREAD_SECS);
    }
    Json(result).into_response()
}

pub(crate) async fn get_thread_reactions(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ThreadReactionsResponse {
            reactions: HashMap::new(),
        });
    }

    let Some(ref pool) = state.pg_pool else {
        return Json(ThreadReactionsResponse {
            reactions: HashMap::new(),
        });
    };

    let rows = sqlx::query_as::<_, (i64, String, i64, bool)>(
        "SELECT message_uid, emoji, COUNT(*) as cnt,
                bool_or(account_address = $1) as me
         FROM reactions
         WHERE thread_id = $2
         GROUP BY message_uid, emoji
         ORDER BY message_uid, emoji",
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

pub(crate) async fn fetch_message_reactions(
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
         ORDER BY emoji",
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

//! Search handlers (keyword + pgvector semantic).

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;


use super::super::{validate_domains, AuthUser, WebState};
use super::*;

pub(crate) async fn search_conversations(
    AuthUser { address: ref user, ref permissions, .. }: AuthUser,
    Query(q): Query<SearchQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ConversationResponse>::new());
    };

    if q.q.len() > crate::web::MAX_QUERY_LEN {
        return Json(Vec::<ConversationResponse>::new());
    }

    let limit = crate::web::clamp_limit(q.limit);
    let domains = validate_domains(q.domains.as_deref(), permissions);

    // try meilisearch first for fast, typo-tolerant search
    let mut convos = if let Some(ref meili) = state.meili {
        match meili.search(&q.q, user, limit * 3).await {
            Ok(thread_ids) if !thread_ids.is_empty() => {
                mb_store
                    .get_conversations_by_thread_ids(user, &thread_ids, domains.as_deref())
                    .await
                    .unwrap_or_default()
            }
            _ => {
                // fall back to PG search
                mb_store
                    .search_conversations(user, &q.q, limit, q.category.as_deref(), domains.as_deref())
                    .await
                    .unwrap_or_default()
            }
        }
    } else {
        mb_store
            .search_conversations(user, &q.q, limit, q.category.as_deref(), domains.as_deref())
            .await
            .unwrap_or_default()
    };

    // supplement with semantic search when text search returns few results
    if convos.len() < limit as usize
        && let Some(extra) = semantic_search_threads(
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

    Json(convos_to_response(convos))
}

pub(crate) async fn semantic_search(
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

    if q.q.len() > crate::web::MAX_QUERY_LEN {
        return Json(Vec::<SemanticSearchResult>::new());
    }

    let limit = crate::web::clamp_limit(q.limit);

    let embedding = match llm_config.embed(&q.q).await {
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

    let embedding = llm.embed(query).await?;
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

        if let Some(filter) = category
            && cat != filter {
                continue;
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
            requires_action: false,
            last_sender: last.sender.clone(),
            sent_count: 0,
        });
    }

    Some(out)
}

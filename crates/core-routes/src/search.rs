//! Conversation search, the same in every store: the words of the query
//! each matched through the store's indexes, every word required, and the
//! matches judged by the list the search was made from and its operators.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::conversation::{
    SearchConversationsRequest, SearchConversationsResponse,
};
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};
use mailrs_mailbox::query::{Cond, Place, Query, ThreadFacts};
use mailrs_mailbox::scope::{self, Bucket, Scope};
use mailrs_mailbox::search::{self, Stage};

use crate::store::{SearchStore, StoreError};

/// POST /v1/users/{user}/conversations:search — the conversations that
/// match every word and every operator, in the list the search was made
/// from or the place `in:` names; best match first, newest first within
/// one, newest first when there are only operators.
pub async fn search_conversations<S: SearchStore>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Json(req): Json<SearchConversationsRequest>,
) -> Response {
    match search(state.as_ref(), &user, &req).await {
        Ok(items) => Json(SearchConversationsResponse { items }).into_response(),
        Err(e) => {
            tracing::error!(error = %e, %user, "search failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub(crate) async fn search<S: SearchStore>(
    state: &S,
    user: &str,
    req: &SearchConversationsRequest,
) -> Result<Vec<ConversationSummaryWire>, StoreError> {
    let query = Query::parse(&req.query);
    let words = search::words(&query.text);
    if words.is_empty() && !query.has_operators() {
        return Ok(Vec::new());
    }
    let listed = match query.within.is_none() && req.reads_a_list_of_its_own() {
        true => Some(state.list_ids(user, req).await?),
        false => None,
    };
    let ranked = match words.is_empty() {
        true => None,
        false => Some(matches(state, user, &words, req.text_hits_wanted()).await?),
    };
    let ids: Vec<String> = match &ranked {
        Some(ranked) => ranked.keys().cloned().collect(),
        None => state.all_ids(user).await?,
    };
    let ids: Vec<String> = match &listed {
        Some(listed) => ids.into_iter().filter(|id| listed.contains(id)).collect(),
        None => ids,
    };
    let mut rows = state.rows(user, &ids).await?;
    rows.sort_by(|a, b| {
        let stage = |r: &ConversationSummaryWire| ranked.as_ref().map(|m| m[&r.thread_id]);
        stage(a)
            .cmp(&stage(b))
            .then_with(|| b.last_date.cmp(&a.last_date))
            .then_with(|| a.thread_id.cmp(&b.thread_id))
    });
    let names = match asks_labels(&query) {
        true => state.label_names(user).await?,
        false => HashMap::new(),
    };
    let mut kept = Vec::new();
    for row in rows {
        if kept.len() >= req.limit() {
            break;
        }
        let in_place = match (query.within, &listed) {
            (Some(within), _) => !row.quarantined && within.admits(place(&row)),
            (None, Some(_)) => true,
            (None, None) => in_folder_list(&row, &req.filter),
        };
        if !in_place {
            continue;
        }
        let messages = match query.needs_messages() {
            true => state.message_facts(user, &row.thread_id).await?,
            false => Vec::new(),
        };
        let labels = names.get(&row.thread_id).cloned().unwrap_or_default();
        if query.admits(&facts(&row, &labels), || messages) {
            kept.push(row);
        }
    }
    Ok(kept)
}

/// The conversations every word matched, each at its weakest word's best
/// stage.
async fn matches<S: SearchStore>(
    state: &S,
    user: &str,
    words: &[String],
    want: usize,
) -> Result<HashMap<String, Stage>, StoreError> {
    let mut per_word = Vec::with_capacity(words.len());
    for word in words {
        let mut hits = state.word_hits(user, word, want).await?;
        let seen: HashSet<String> = hits.iter().map(|(id, _)| id.clone()).collect();
        if word.chars().count() >= 2 {
            for id in state.substring_hits(user, word, &seen, want).await? {
                hits.push((id, Stage::Substring));
            }
        }
        per_word.push(hits);
    }
    Ok(search::every_word(&per_word).into_iter().collect())
}

fn asks_labels(query: &Query) -> bool {
    query.conds.iter().any(|(_, c)| matches!(c, Cond::Label(_)))
}

fn sent_only(row: &ConversationSummaryWire) -> bool {
    row.message_count > 0 && row.sent_count >= row.message_count
}

fn place(row: &ConversationSummaryWire) -> Place {
    Place {
        archived: row.archived,
        trashed: row.trashed_at > 0,
        junk: scope::bucket_of(&row.category) == Bucket::Junk,
        has_sent: row.sent_count > 0,
        sent_only: sent_only(row),
        starred: row.flagged,
    }
}

fn facts<'a>(row: &'a ConversationSummaryWire, labels: &'a [String]) -> ThreadFacts<'a> {
    ThreadFacts {
        subject: &row.subject,
        senders: &row.participants,
        latest_date: row.last_date,
        unread_count: i64::from(row.unread_count),
        starred: row.flagged,
        pinned: row.pinned,
        labels,
    }
}

/// Whether the folder's list `f` shows this row: predicate for predicate
/// what the list asks its store, read from the row the list would show.
fn in_folder_list(row: &ConversationSummaryWire, f: &ConversationFilter) -> bool {
    // Archived is a tab across folders, not a predicate inside one
    let folder = match f.archived {
        true => None,
        false => f.folder.as_deref(),
    };
    let flagged = f.starred == Some(true) || f.pinned == Some(true);
    let scope = scope::list_scope(
        folder,
        f.category.as_deref(),
        f.unread == Some(true),
        flagged,
    );
    // All Mail and the lists narrowed from it read both sides of the archive
    if scope != Scope::AllMail && row.archived != f.archived {
        return false;
    }
    if row.trashed_at > 0 {
        return false;
    }
    // held mail is out of every list unless the reader shows it in place
    if row.quarantined && !f.include_quarantined {
        return false;
    }
    if f.unread == Some(true) && row.unread_count == 0 {
        return false;
    }
    if f.starred == Some(true) && !row.flagged {
        return false;
    }
    if f.accounts
        .as_ref()
        .is_some_and(|ids| !ids.contains(&row.account_id))
    {
        return false;
    }
    if scope::is_sent_folder(folder) && row.sent_count == 0 {
        return false;
    }
    scope.admits(scope::ThreadFacts {
        category: &row.category,
        sent_only: sent_only(row),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use mailrs_mailbox::query::MessageFacts;

    use super::*;
    use crate::store::StoreResult;

    /// A store every read of which fails.
    struct Broken;

    fn broken<T>() -> StoreResult<T> {
        Err(StoreError("the store is down".into()))
    }

    impl SearchStore for Broken {
        async fn word_hits(&self, _: &str, _: &str, _: usize) -> StoreResult<Vec<(String, Stage)>> {
            broken()
        }
        async fn substring_hits(
            &self,
            _: &str,
            _: &str,
            _: &HashSet<String>,
            _: usize,
        ) -> StoreResult<Vec<String>> {
            broken()
        }
        async fn list_ids(
            &self,
            _: &str,
            _: &SearchConversationsRequest,
        ) -> StoreResult<HashSet<String>> {
            broken()
        }
        async fn all_ids(&self, _: &str) -> StoreResult<Vec<String>> {
            broken()
        }
        async fn rows(&self, _: &str, _: &[String]) -> StoreResult<Vec<ConversationSummaryWire>> {
            broken()
        }
        async fn message_facts(&self, _: &str, _: &str) -> StoreResult<Vec<MessageFacts>> {
            broken()
        }
        async fn label_names(&self, _: &str) -> StoreResult<HashMap<String, Vec<String>>> {
            broken()
        }
    }

    /// A store that cannot answer is an error the reader sees, not an
    /// empty or partial list that reads as "nothing matched".
    #[tokio::test]
    async fn a_store_that_fails_is_a_500_not_no_results() {
        for query in ["invoice", "has:attachment", "invoice in:archive"] {
            let req = SearchConversationsRequest {
                query: query.into(),
                ..Default::default()
            };
            let res = search_conversations(
                State(Arc::new(Broken)),
                Path("a@x.test".to_string()),
                Json(req),
            )
            .await;
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR, "{query}");
        }
    }

    /// A query with neither words nor operators asks the store nothing.
    #[tokio::test]
    async fn an_empty_query_finds_nothing_without_asking() {
        let req = SearchConversationsRequest {
            query: " .. ".into(),
            ..Default::default()
        };
        let res =
            search_conversations(State(Arc::new(Broken)), Path("a@x.test".into()), Json(req)).await;
        assert_eq!(res.status(), StatusCode::OK);
    }
}

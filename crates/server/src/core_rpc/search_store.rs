//! The SQL store's side of search, for the shared route: the words the
//! application cut into the message rows and attachments, and a substring
//! scan of subjects, senders and previews.

use std::collections::{HashMap, HashSet};

use mailrs_core_api::method::conversation::{LIST_SCOPE_CAP, SearchConversationsRequest};
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};
use mailrs_core_routes::store::{SearchStore, StoreError, StoreResult};
use mailrs_mailbox::pg::{Held, ThreadQuery};
use mailrs_mailbox::query::MessageFacts;
use mailrs_mailbox::search::Stage;

use super::CoreRpcState;
use super::handlers::conversation::{labelled_query, thread_query};

fn failed(e: sqlx::Error) -> StoreError {
    StoreError(e.to_string())
}

fn cap(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

impl SearchStore for CoreRpcState {
    async fn word_hits(
        &self,
        user: &str,
        word: &str,
        want: usize,
    ) -> StoreResult<Vec<(String, Stage)>> {
        self.mailbox
            .word_threads(user, word, cap(want))
            .await
            .map_err(failed)
    }

    async fn substring_hits(
        &self,
        user: &str,
        needle: &str,
        skip: &HashSet<String>,
        want: usize,
    ) -> StoreResult<Vec<String>> {
        self.mailbox
            .substring_threads(user, needle, skip, want)
            .await
            .map_err(failed)
    }

    async fn list_ids(
        &self,
        user: &str,
        req: &SearchConversationsRequest,
    ) -> StoreResult<HashSet<String>> {
        let whole = ConversationFilter {
            before_ts: None,
            after_cursor: None,
            limit: u32::try_from(LIST_SCOPE_CAP).unwrap_or(u32::MAX),
            ..req.filter.clone()
        };
        let labelled = match (&whole.label, req.quarantined) {
            (Some(id), false) => Some(
                self.mailbox
                    .threads_with_label(user, id)
                    .await
                    .map_err(|e| StoreError(e.to_string()))?,
            ),
            _ => None,
        };
        let q = match (req.quarantined, labelled.as_deref()) {
            // the Review list, as `list_quarantined` reads it
            (true, _) => ThreadQuery {
                held: Held::Only,
                limit: Some(cap(LIST_SCOPE_CAP)),
                ..ThreadQuery::all(user)
            },
            (false, Some(ids)) => labelled_query(thread_query(user, &whole), ids),
            (false, None) => thread_query(user, &whole),
        };
        let rows = self.mailbox.thread_rows(&q).await.map_err(failed)?;
        Ok(rows.into_iter().map(|r| r.thread_id).collect())
    }

    async fn all_ids(&self, user: &str) -> StoreResult<Vec<String>> {
        let q = ThreadQuery {
            archived: None,
            trashed: None,
            held: Held::Include,
            ..ThreadQuery::all(user)
        };
        let rows = self.mailbox.thread_rows(&q).await.map_err(failed)?;
        Ok(rows.into_iter().map(|r| r.thread_id).collect())
    }

    async fn rows(&self, user: &str, ids: &[String]) -> StoreResult<Vec<ConversationSummaryWire>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let q = ThreadQuery {
            archived: None,
            trashed: None,
            held: Held::Include,
            thread_ids: Some(ids),
            ..ThreadQuery::all(user)
        };
        let rows = self.mailbox.thread_rows(&q).await.map_err(failed)?;
        let items = rows.iter().map(Into::into).collect();
        Ok(super::handlers::labels::with_labels(self, user, items).await)
    }

    async fn message_facts(&self, user: &str, thread_id: &str) -> StoreResult<Vec<MessageFacts>> {
        self.mailbox
            .message_facts(user, thread_id)
            .await
            .map_err(failed)
    }

    async fn label_names(&self, user: &str) -> StoreResult<HashMap<String, Vec<String>>> {
        let names: HashMap<String, String> = self
            .mailbox
            .labels(user)
            .await
            .map_err(failed)?
            .into_iter()
            .map(|l| (l.id, l.name))
            .collect();
        Ok(self
            .mailbox
            .labels_by_thread(user)
            .await
            .map_err(failed)?
            .into_iter()
            .map(|(tid, ids)| {
                let named = ids.iter().filter_map(|id| names.get(id).cloned()).collect();
                (tid, named)
            })
            .collect())
    }
}

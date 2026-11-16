//! The kevy store's side of search, for the shared route: the three text
//! indexes kevy keeps from its commit hook — the reader's own rows, the
//! message bodies, the attachments' text — and a substring walk over the
//! reader's rows.

use std::collections::{HashMap, HashSet};

use mailrs_core_api::method::conversation::{LIST_SCOPE_CAP, SearchConversationsRequest};
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};
use mailrs_core_routes::store::{SearchStore, StoreResult};
use mailrs_mailbox::query::MessageFacts;
use mailrs_mailbox::search::Stage;
use mailrs_mailbox_kevy::{ListThreadsFilter, QuarantineScope};

use crate::FastcoreState;

impl SearchStore for FastcoreState {
    async fn word_hits(
        &self,
        user: &str,
        word: &str,
        want: usize,
    ) -> StoreResult<Vec<(String, Stage)>> {
        let mut hits: Vec<(String, Stage)> = Vec::new();
        for (tid, _) in self.mailbox.search_threads(user, word, want)? {
            hits.push((tid, Stage::Header));
        }
        for tid in self.mailbox.search_message_bodies(user, word, want)? {
            hits.push((tid, Stage::Body));
        }
        for tid in self.mailbox.search_attachment_texts(user, word, want)? {
            hits.push((tid, Stage::Attachment));
        }
        Ok(hits)
    }

    async fn substring_hits(
        &self,
        user: &str,
        needle: &str,
        skip: &HashSet<String>,
        want: usize,
    ) -> StoreResult<Vec<String>> {
        Ok(self.mailbox.like_threads(user, needle, skip, want)?)
    }

    async fn list_ids(
        &self,
        user: &str,
        req: &SearchConversationsRequest,
    ) -> StoreResult<HashSet<String>> {
        let rows = match req.quarantined {
            // the Review list, as `list_quarantined` reads it
            true => {
                let filter = ListThreadsFilter {
                    quarantine: QuarantineScope::Only,
                    ..Default::default()
                };
                self.mailbox
                    .list_threads_by_activity(user, &filter, 0, LIST_SCOPE_CAP)?
                    .0
            }
            false => {
                let whole = ConversationFilter {
                    before_ts: None,
                    after_cursor: None,
                    ..req.filter.clone()
                };
                crate::routes::list_rows(self, user, &whole, LIST_SCOPE_CAP)?
            }
        };
        Ok(rows.into_iter().map(|r| r.thread_id).collect())
    }

    async fn all_ids(&self, user: &str) -> StoreResult<Vec<String>> {
        Ok(self.mailbox.all_thread_ids_for_user(user)?)
    }

    async fn rows(&self, user: &str, ids: &[String]) -> StoreResult<Vec<ConversationSummaryWire>> {
        let mut rows = Vec::with_capacity(ids.len());
        for tid in ids {
            if let Some(row) = self.mailbox.get_thread_for_user(user, tid)? {
                rows.push(crate::routes::message_ops::row_to_wire(row));
            }
        }
        let rows = crate::routes::labels::with_labels(self, user, rows);
        let rows = crate::sections::with_sections(self, user, rows);
        Ok(crate::shared::with_shared(self, user, rows))
    }

    async fn message_facts(&self, user: &str, thread_id: &str) -> StoreResult<Vec<MessageFacts>> {
        let mut out = Vec::new();
        for blob in self.mailbox.list_thread_messages(user, thread_id)? {
            // skipped as the thread's message list skips it
            let wire: mailrs_core_api::method::message::MessageWire =
                match serde_json::from_slice(&blob) {
                    Ok(w) => w,
                    Err(e) => {
                        tracing::warn!(%user, %thread_id, error = %e, "undecodable message row");
                        continue;
                    }
                };
            out.push(MessageFacts {
                sender: wire.sender,
                recipients: wire.recipients,
                subject: wire.subject,
                attachments: wire.attachments,
            });
        }
        Ok(out)
    }

    async fn label_names(&self, user: &str) -> StoreResult<HashMap<String, Vec<String>>> {
        let names: HashMap<String, String> = self
            .mailbox
            .labels(user)?
            .into_iter()
            .map(|l| (l.id, l.name))
            .collect();
        Ok(self
            .mailbox
            .labels_by_thread(user)?
            .into_iter()
            .map(|(tid, ids)| {
                let named = ids.iter().filter_map(|id| names.get(id).cloned()).collect();
                (tid, named)
            })
            .collect())
    }
}

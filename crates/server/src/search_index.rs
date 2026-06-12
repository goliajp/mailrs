//! meilisearch integration for full-text email search
//!
//! indexes messages into meilisearch for fast, typo-tolerant search with
//! good CJK tokenization. falls back to PG search when meili is unavailable.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const INDEX_NAME: &str = "messages";
const BATCH_SIZE: usize = 1000;
// poll for new messages every 30 s once caught up. while a backfill batch
// is still returning rows, keep going with a short pause so the initial
// import doesn't take 7+ hours on a 20k-message mailbox.
const IDLE_INTERVAL: Duration = Duration::from_secs(30);
const ACTIVE_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone)]
pub struct MeiliClient {
    url: String,
    key: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
pub(crate) struct MeiliDocument {
    id: i64,
    thread_id: String,
    subject: String,
    sender: String,
    /// MRS-1 hotfix: searching `qualcomm` would miss any inbound mail from
    /// `@qualcomm.com` because only `sender` was indexed (and the user's
    /// own address was the one ending up in `recipients`). Add it
    /// explicitly so address-fragment search hits both directions.
    recipients: String,
    text_body: String,
    clean_text: String,
    internal_date: i64,
    user_address: String,
}

#[derive(Deserialize)]
struct MeiliSearchResponse {
    hits: Vec<MeiliHit>,
}

#[derive(Deserialize)]
pub struct MeiliHit {
    pub thread_id: String,
}

impl MeiliClient {
    pub fn new(url: String, key: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self { url, key, http }
    }

    async fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.url, path))
            .header("Authorization", format!("Bearer {}", self.key))
    }

    /// configure index settings (filterable/sortable attributes, CJK tokenizer)
    pub async fn configure_index(&self) -> Result<(), String> {
        // create index
        let _ = self
            .request(reqwest::Method::POST, "/indexes")
            .await
            .json(&serde_json::json!({
                "uid": INDEX_NAME,
                "primaryKey": "id"
            }))
            .send()
            .await;

        // set filterable attributes
        let _ = self
            .request(
                reqwest::Method::PUT,
                &format!("/indexes/{INDEX_NAME}/settings/filterable-attributes"),
            )
            .await
            .json(&["user_address", "thread_id"])
            .send()
            .await;

        // set sortable attributes
        let _ = self
            .request(
                reqwest::Method::PUT,
                &format!("/indexes/{INDEX_NAME}/settings/sortable-attributes"),
            )
            .await
            .json(&["internal_date"])
            .send()
            .await;

        // set searchable attributes
        let _ = self
            .request(
                reqwest::Method::PUT,
                &format!("/indexes/{INDEX_NAME}/settings/searchable-attributes"),
            )
            .await
            .json(&["subject", "sender", "recipients", "clean_text", "text_body"])
            .send()
            .await;

        // Override ranking-rules to put `sort` first (default is fifth).
        // Without this, "qualcomm" matches in subject outrank matches in
        // sender / clean_text — so a newer email from a @qualcomm.com
        // address gets buried behind older emails whose subject happens to
        // contain Qualcomm. For an email client the "newest first" axis
        // matters more than relevance — we still keep words/typo/proximity
        // for tiebreaking among same-time hits.
        let _ = self
            .request(
                reqwest::Method::PUT,
                &format!("/indexes/{INDEX_NAME}/settings/ranking-rules"),
            )
            .await
            .json(&[
                "sort",
                "words",
                "typo",
                "proximity",
                "attribute",
                "exactness",
            ])
            .send()
            .await;

        Ok(())
    }

    /// index a batch of documents
    pub(crate) async fn index_documents(&self, docs: &[MeiliDocument]) -> Result<(), String> {
        self.request(
            reqwest::Method::POST,
            &format!("/indexes/{INDEX_NAME}/documents"),
        )
        .await
        .json(docs)
        .send()
        .await
        .map_err(|e| format!("meili index error: {e}"))?;
        Ok(())
    }

    /// search for matching thread_ids
    pub async fn search(&self, query: &str, user: &str, limit: u32) -> Result<Vec<String>, String> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/indexes/{INDEX_NAME}/search"),
            )
            .await
            .json(&serde_json::json!({
                "q": query,
                "filter": format!("user_address = \"{}\"", user.replace('"', "\\\"")),
                "limit": limit,
                "sort": ["internal_date:desc"],
                "attributesToRetrieve": ["thread_id"],
            }))
            .send()
            .await
            .map_err(|e| format!("meili search error: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("meili search returned {}", resp.status()));
        }

        let result: MeiliSearchResponse = resp
            .json()
            .await
            .map_err(|e| format!("meili parse error: {e}"))?;

        // deduplicate thread_ids while preserving order
        let mut seen = std::collections::HashSet::new();
        let thread_ids: Vec<String> = result
            .hits
            .into_iter()
            .filter_map(|h| {
                if seen.insert(h.thread_id.clone()) {
                    Some(h.thread_id)
                } else {
                    None
                }
            })
            .collect();

        Ok(thread_ids)
    }
}

/// kevy key holding the highest message id already pushed to meili
const CHECKPOINT_KEY: &[u8] = b"meili:last_indexed_id";

/// read the indexer checkpoint from kevy. absent, unreadable, or
/// unparseable values all mean "no checkpoint": start from 0 and let
/// meili's id-based deduplication absorb the re-scan — exactly the
/// pre-checkpoint first-run semantics
fn load_checkpoint(store: &kevy_embedded::Store) -> i64 {
    let raw = match store.get(CHECKPOINT_KEY) {
        Ok(Some(raw)) => raw,
        Ok(None) => return 0,
        Err(e) => {
            tracing::warn!(event = "meili_checkpoint_read_failed", error = %e);
            return 0;
        }
    };
    match std::str::from_utf8(&raw).ok().and_then(|s| s.parse().ok()) {
        Some(id) => id,
        None => {
            tracing::warn!(event = "meili_checkpoint_unparseable", raw = ?raw);
            0
        }
    }
}

/// persist the indexer checkpoint to kevy. a write failure only costs
/// resume-after-restart, so log and keep indexing
fn save_checkpoint(store: &kevy_embedded::Store, id: i64) {
    if let Err(e) = store.set(CHECKPOINT_KEY, id.to_string().as_bytes()) {
        tracing::warn!(event = "meili_checkpoint_write_failed", error = %e);
    }
}

/// spawn background indexer that syncs messages from PG to meilisearch
pub fn spawn_indexer(
    client: Arc<MeiliClient>,
    pool: crate::pg::BackendPool,
    kevy: Option<crate::kevy_store::KevyStore>,
) {
    tokio::spawn(async move {
        // configure index on startup
        if let Err(e) = client.configure_index().await {
            tracing::error!(event = "meili_configure_failed", error = %e);
        }

        // resume from the kevy checkpoint; without kevy (degraded boot)
        // start from 0 and rely on meili's id-based deduplication to
        // skip rows already imported
        let mut last_id: i64 = kevy.as_deref().map(load_checkpoint).unwrap_or(0);
        if last_id > 0 {
            tracing::info!(event = "meili_resume_from_checkpoint", last_id);
        }

        loop {
            // fetch unindexed messages from PG
            type MessageRow = (
                i64,
                String,
                Option<String>,
                String,
                String,
                Option<String>,
                Option<String>,
                i64,
                String,
            );
            let rows: Vec<MessageRow> = match sqlx::query_as(
                "SELECT m.id, m.thread_id, m.subject, m.sender, m.recipients, m.text_body, m.clean_text, m.internal_date, mb.user_address \
                 FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id \
                 WHERE m.id > $1 \
                 ORDER BY m.id ASC LIMIT $2"
            )
                .bind(last_id)
                .bind(BATCH_SIZE as i64)
                .fetch_all(&pool)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(event = "meili_backfill_query_failed", error = %e);
                    tokio::time::sleep(IDLE_INTERVAL).await;
                    continue;
                }
            };

            if rows.is_empty() {
                // caught up — wait longer before polling again
                tokio::time::sleep(IDLE_INTERVAL).await;
                continue;
            }

            let max_id = rows.last().map(|r| r.0).unwrap_or(last_id);
            let row_count = rows.len();

            let docs: Vec<MeiliDocument> = rows
                .into_iter()
                .map(
                    |(
                        id,
                        thread_id,
                        subject,
                        sender,
                        recipients,
                        text_body,
                        clean_text,
                        internal_date,
                        user_address,
                    )| {
                        MeiliDocument {
                            id,
                            thread_id,
                            subject: subject.unwrap_or_default(),
                            sender,
                            recipients,
                            text_body: text_body.unwrap_or_default(),
                            clean_text: clean_text.unwrap_or_default(),
                            internal_date,
                            user_address,
                        }
                    },
                )
                .collect();

            let count = docs.len();
            match client.index_documents(&docs).await {
                Ok(()) => {
                    last_id = max_id;
                    if let Some(store) = &kevy {
                        save_checkpoint(store, last_id);
                    }
                    if count > 0 {
                        tracing::info!(event = "meili_indexed_batch", count, max_id);
                    }
                }
                Err(e) => {
                    tracing::error!(event = "meili_index_failed", error = %e);
                }
            }

            // if the batch was full there's likely more — loop with a tiny
            // pause to let other queries breathe. once a partial batch comes
            // back we know we caught up and switch to the idle interval.
            if row_count == BATCH_SIZE {
                tokio::time::sleep(ACTIVE_INTERVAL).await;
            } else {
                tokio::time::sleep(IDLE_INTERVAL).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_store() -> kevy_embedded::Store {
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap()
    }

    #[test]
    fn checkpoint_absent_means_zero() {
        let store = memory_store();

        assert_eq!(load_checkpoint(&store), 0);
    }

    #[test]
    fn checkpoint_roundtrip() {
        let store = memory_store();

        save_checkpoint(&store, 24304);

        assert_eq!(load_checkpoint(&store), 24304);
    }

    #[test]
    fn checkpoint_unparseable_means_zero() {
        let store = memory_store();
        store.set(CHECKPOINT_KEY, b"not-a-number").unwrap();

        assert_eq!(load_checkpoint(&store), 0);
    }
}

//! Cutting the search words of mail recorded before they were kept, once,
//! at startup: a batch at a time until none is left. On a store already
//! filled that is one query finding nothing.

use std::sync::Arc;

use mailrs_mailbox::PgMailboxStore;

const BATCH: i64 = 500;

pub(crate) fn spawn_fill(store: Arc<PgMailboxStore>) {
    tokio::spawn(async move {
        let mut total = 0;
        loop {
            match store.fill_search_words(BATCH).await {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(e) => {
                    tracing::error!(error = %e, total, "filling search words failed");
                    return;
                }
            }
        }
        if total > 0 {
            tracing::info!(total, "search words filled in");
        }
    });
}

//! The SQL store's side of held mail, for the shared routes.

use mailrs_core_api::method::thread::ReleaseFact;
use mailrs_core_routes::store::{HoldStore, StoreError, StoreResult};
use mailrs_mailbox::pg::{Held, ThreadQuery};

use super::CoreRpcState;

fn failed(e: sqlx::Error) -> StoreError {
    StoreError(e.to_string())
}

impl HoldStore for CoreRpcState {
    async fn held(&self, user: &str, thread_id: &str) -> StoreResult<Option<bool>> {
        let ids = [thread_id.to_string()];
        let q = ThreadQuery {
            archived: None,
            held: Held::Include,
            thread_ids: Some(&ids),
            ..ThreadQuery::all(user)
        };
        let rows = self.mailbox.thread_rows(&q).await.map_err(failed)?;
        Ok(rows.into_iter().next().map(|r| r.quarantined))
    }

    async fn release_by_person(&self, user: &str, thread_id: &str) -> StoreResult<()> {
        self.mailbox
            .release_thread(user, thread_id)
            .await
            .map_err(failed)?;
        let fact = mailrs_mailbox::pg::ReleaseFact {
            at: chrono::Utc::now().timestamp(),
            source: "person".to_string(),
            actor: user.to_string(),
            rules_version: String::new(),
        };
        self.mailbox
            .record_release(user, thread_id, &fact)
            .await
            .map_err(failed)?;
        // the audit line fastcore's release record writes, alike
        let detail = format!(
            "user={user} source={} actor={} rules_version={}",
            fact.source, fact.actor, fact.rules_version
        );
        mailrs_core_sidestate::families::audit::record_by_core(
            self,
            &fact.actor,
            "quarantine.release",
            thread_id,
            &detail,
            fact.at,
        );
        Ok(())
    }

    async fn queue_images(&self, user: &str, thread_id: &str) {
        let Some(url) = self.net_url.clone() else {
            return;
        };
        match self
            .mailbox
            .thread_raw_messages(user, thread_id, &self.maildir_root)
            .await
        {
            Ok(raws) => {
                let q = mailrs_intake::image_prefetch::NetworkQueue { url };
                mailrs_intake::image_prefetch::queue_thread(&q, user, thread_id, &raws);
            }
            Err(e) => tracing::warn!(error = %e, %user, %thread_id, "images not queued"),
        }
    }

    async fn releases(&self, user: &str, thread_id: &str) -> StoreResult<Vec<ReleaseFact>> {
        let facts = self
            .mailbox
            .releases(user, thread_id)
            .await
            .map_err(failed)?;
        Ok(facts
            .into_iter()
            .map(|f| ReleaseFact {
                at: f.at,
                source: f.source,
                actor: f.actor,
                rules_version: f.rules_version,
            })
            .collect())
    }
}

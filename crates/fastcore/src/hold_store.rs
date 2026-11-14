//! The kevy store's side of held mail, for the shared routes.
//!
//! Held mail is neither Junk nor deleted. It stays in the maildir because
//! it is the evidence the abuse reports are built from, and out of every
//! ordinary list because a reader should not run into an attempt to defraud
//! them by accident.

use mailrs_core_api::method::thread::ReleaseFact;
use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_core_routes::store::{HoldStore, StoreError, StoreResult};
use mailrs_mailbox_kevy::{ListThreadsFilter, QuarantineScope};

use crate::FastcoreState;

impl HoldStore for FastcoreState {
    async fn held_page(
        &self,
        user: &str,
        before_ts: Option<i64>,
        n: usize,
    ) -> StoreResult<(Vec<ConversationSummaryWire>, usize)> {
        let filter = ListThreadsFilter {
            quarantine: QuarantineScope::Only,
            before_ts,
            ..Default::default()
        };
        // the count comes out of the same index walk as the rows
        let (rows, total) = self.mailbox.list_threads_by_activity(user, &filter, 0, n)?;
        let items = rows
            .into_iter()
            .map(crate::routes::message_ops::row_to_wire)
            .collect();
        let items = crate::routes::labels::with_labels(self, user, items);
        Ok((crate::sections::with_sections(self, user, items), total))
    }

    async fn held(&self, user: &str, thread_id: &str) -> StoreResult<Option<bool>> {
        Ok(self
            .mailbox
            .get_thread_for_user(user, thread_id)?
            .map(|t| t.quarantined))
    }

    async fn release_by_person(&self, user: &str, thread_id: &str) -> StoreResult<()> {
        self.mailbox.set_quarantined(user, thread_id, false)?;
        // the fact a later re-judge consults: what was here when this person
        // let it go is not held again
        crate::hold_facts::record_release(
            self,
            user,
            thread_id,
            crate::hold_facts::PERSON,
            user,
            "",
            "",
        )
        .map_err(StoreError)?;
        // re-judging takes it again only for a rule it was not put back despite
        crate::rejudge::recall::note_released(self, user, thread_id);
        Ok(())
    }

    async fn queue_images(&self, user: &str, thread_id: &str) {
        crate::image_prefetch::queue_thread(self, user, thread_id);
    }

    async fn releases(&self, user: &str, thread_id: &str) -> StoreResult<Vec<ReleaseFact>> {
        crate::hold_facts::releases(self, user, thread_id).map_err(StoreError)
    }

    async fn fraud_verdict(
        &self,
        user: &str,
        message_id: &str,
    ) -> StoreResult<Option<Option<serde_json::Value>>> {
        if self.mailbox.user_message_facts(user, message_id)?.is_none() {
            return Ok(None);
        }
        let verdict = self
            .mailbox
            .fraud_verdict(message_id)?
            .and_then(|raw| serde_json::from_str(&raw).ok());
        let placement = self
            .mailbox
            .message_placement(user, message_id)?
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Ok(Some(mailrs_intake::why::joined(verdict, placement)))
    }
}

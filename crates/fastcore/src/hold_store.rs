//! The kevy store's side of held mail, for the shared routes.

use mailrs_core_api::method::thread::ReleaseFact;
use mailrs_core_routes::store::{HoldStore, StoreError, StoreResult};

use crate::FastcoreState;

impl HoldStore for FastcoreState {
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
}

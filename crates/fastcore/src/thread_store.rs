//! The kevy store's side of a reader's acts on a conversation, for the
//! shared routes: each act writes the membership row, every message of
//! the reader's, the maildir file names, and the keyword bits and state
//! log a rebuilt index reads back.

use mailrs_core_routes::store::{ReaderView, StoreError, StoreResult, ThreadStore};

use crate::FastcoreState;
use crate::routes::thread_actions::{mark_thread_read_everywhere, set_archived_state};

fn failed(e: std::io::Error) -> StoreError {
    StoreError(e.to_string())
}

impl ThreadStore for FastcoreState {
    async fn reader_view(&self, user: &str, thread_id: &str) -> StoreResult<Option<ReaderView>> {
        let Some(row) = self
            .mailbox
            .get_thread_for_user(user, thread_id)
            .map_err(failed)?
        else {
            return Ok(None);
        };
        let latest = self
            .mailbox
            .get_thread(thread_id)
            .map_err(failed)?
            .map_or(0, |r| r.latest_date);
        Ok(Some(ReaderView {
            unread: row.unread_count > 0,
            latest,
            // the people who wrote to this reader, not everyone on it
            participants: row.senders_csv,
        }))
    }

    async fn mark_read(&self, user: &str, thread_id: &str) -> StoreResult<bool> {
        Ok(mark_thread_read_everywhere(self, user, thread_id))
    }

    async fn mark_unread(&self, user: &str, thread_id: &str) -> StoreResult<bool> {
        if !self.mailbox.mark_unread(user, thread_id).map_err(failed)? {
            return Ok(false);
        }
        // the newest message is the one that is unread again, in its row and
        // in its file name, which is what an IMAP client reads
        if let Some((blob_ref, flags)) = self
            .mailbox
            .mark_newest_message_unseen(user, thread_id)
            .map_err(failed)?
            && !blob_ref.is_empty()
            && let Err(e) = crate::maildir_scan::apply_flag_bitmask(user, &blob_ref, flags)
        {
            tracing::warn!(error = %e, %user, %blob_ref, "mark unread: rename failed");
        }
        Ok(true)
    }

    async fn set_starred(&self, user: &str, thread_id: &str, on: bool) -> StoreResult<bool> {
        self.mailbox
            .set_starred(user, thread_id, on)
            .map_err(failed)
    }

    async fn set_pinned(&self, user: &str, thread_id: &str, on: bool) -> StoreResult<bool> {
        let member = self
            .mailbox
            .set_pinned(user, thread_id, on)
            .map_err(failed)?;
        // a person's decision, written where a rebuilt index cannot lose it
        if member {
            crate::keywords::set_on_thread(self, user, thread_id, crate::keywords::PINNED, on);
        }
        Ok(member)
    }

    async fn set_archived(&self, user: &str, thread_id: &str, on: bool) -> StoreResult<bool> {
        set_archived_state(self, user, thread_id, on).map_err(failed)
    }

    async fn set_snoozed(&self, user: &str, thread_id: &str, until: i64) -> StoreResult<()> {
        self.mailbox
            .set_snoozed(user, thread_id, until)
            .map_err(failed)?;
        // a snooze carries a time, so no flag or keyword bit can hold it: it
        // goes in the log beside the mail, where a rebuilt index reads it.
        // Zero is a value here, not an absence: it brings the thread back.
        let mut rec = crate::threadstate::about(thread_id);
        rec.snoozed_until = Some(until);
        crate::threadstate::record(user, &rec);
        Ok(())
    }
}

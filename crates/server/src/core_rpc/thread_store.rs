//! The SQL store's side of a reader's acts on a conversation, for the
//! shared routes. A conversation the reader is not in is answered before
//! anything is written.

use mailrs_core_routes::store::{ReaderView, StoreError, StoreResult, ThreadStore};
use mailrs_mailbox::pg::{Held, ThreadQuery};

use super::CoreRpcState;

fn failed(e: sqlx::Error) -> StoreError {
    StoreError(e.to_string())
}

impl CoreRpcState {
    async fn member(&self, user: &str, thread_id: &str) -> StoreResult<bool> {
        self.mailbox
            .user_in_thread(user, thread_id)
            .await
            .map_err(failed)
    }
}

impl ThreadStore for CoreRpcState {
    async fn reader_view(&self, user: &str, thread_id: &str) -> StoreResult<Option<ReaderView>> {
        let ids = [thread_id.to_string()];
        let q = ThreadQuery {
            archived: None,
            held: Held::Include,
            thread_ids: Some(&ids),
            ..ThreadQuery::all(user)
        };
        let rows = self.mailbox.thread_rows(&q).await.map_err(failed)?;
        Ok(rows.into_iter().next().map(|r| ReaderView {
            unread: r.unread_count > 0,
            latest: r.last_date,
            participants: r.participants,
        }))
    }

    async fn mark_read(&self, user: &str, thread_id: &str) -> StoreResult<bool> {
        if !self.member(user, thread_id).await? {
            return Ok(false);
        }
        self.mailbox
            .mark_thread_read(user, thread_id, None)
            .await
            .map_err(failed)?;
        Ok(true)
    }

    async fn mark_unread(&self, user: &str, thread_id: &str) -> StoreResult<bool> {
        if !self.member(user, thread_id).await? {
            return Ok(false);
        }
        self.mailbox
            .mark_thread_unread(user, thread_id)
            .await
            .map_err(failed)?;
        Ok(true)
    }

    async fn set_starred(&self, user: &str, thread_id: &str, on: bool) -> StoreResult<bool> {
        if !self.member(user, thread_id).await? {
            return Ok(false);
        }
        let done = match on {
            true => self.mailbox.star_thread(user, thread_id).await,
            false => self.mailbox.unstar_thread(user, thread_id).await,
        };
        done.map_err(failed)?;
        Ok(true)
    }

    async fn set_pinned(&self, user: &str, thread_id: &str, on: bool) -> StoreResult<bool> {
        if !self.member(user, thread_id).await? {
            return Ok(false);
        }
        let done = match on {
            true => self.mailbox.pin_thread(user, thread_id).await,
            false => self.mailbox.unpin_thread(user, thread_id).await,
        };
        done.map_err(failed)?;
        Ok(true)
    }

    async fn set_archived(&self, user: &str, thread_id: &str, on: bool) -> StoreResult<bool> {
        if !self.member(user, thread_id).await? {
            return Ok(false);
        }
        let done = match on {
            true => self.mailbox.archive_thread(user, thread_id).await,
            false => self.mailbox.unarchive_thread(user, thread_id).await,
        };
        done.map_err(failed)?;
        Ok(true)
    }

    async fn set_snoozed(&self, user: &str, thread_id: &str, until: i64) -> StoreResult<()> {
        if until == 0 {
            return self
                .mailbox
                .unsnooze_thread(user, thread_id)
                .await
                .map_err(failed);
        }
        use chrono::TimeZone;
        let at = chrono::Utc
            .timestamp_opt(until, 0)
            .single()
            .ok_or_else(|| StoreError(format!("not a time: {until}")))?;
        self.mailbox
            .snooze_thread(user, thread_id, at)
            .await
            .map_err(failed)
    }
}

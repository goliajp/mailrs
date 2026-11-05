//! Team inboxes in SQL: each member's copy of a team conversation with the
//! team's assignee on it, and the reply claim, decided under a row lock so
//! two members' claims on the same count cannot both win.

use crate::pg::PgMailboxStore;
use crate::shared_reply::{self, Claim, ReplyState};

impl PgMailboxStore {
    /// Mark `user`'s thread as their copy of team conversation `tag`,
    /// assigned to `assignee` (empty for nobody).
    pub async fn set_shared_copy(
        &self,
        user: &str,
        thread_id: &str,
        tag: &str,
        assignee: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO shared_thread_copies (account_address, thread_id, shared_conv, assignee)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (account_address, thread_id)
             DO UPDATE SET shared_conv = EXCLUDED.shared_conv, assignee = EXCLUDED.assignee",
        )
        .bind(user)
        .bind(thread_id)
        .bind(tag)
        .bind(assignee)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Copy the team's assignee onto `user`'s thread.
    pub async fn set_assignee(
        &self,
        user: &str,
        thread_id: &str,
        assignee: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE shared_thread_copies SET assignee = $3
             WHERE account_address = $1 AND thread_id = $2 AND assignee <> $3",
        )
        .bind(user)
        .bind(thread_id)
        .bind(assignee)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The reply claims of every conversation of group `gid`, counted, and
    /// removed unless `dry_run`: part of a deleted group's team state. Run
    /// once per purged group, never on a request path.
    pub async fn purge_group_replies(&self, gid: i64, dry_run: bool) -> Result<u64, sqlx::Error> {
        // a tag is "{gid}:{conv}": the colon keeps group 4 from taking 40's
        let prefix = format!("{gid}:%");
        if dry_run {
            let (n,): (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM shared_reply_state WHERE shared_conv LIKE $1")
                    .bind(&prefix)
                    .fetch_one(&self.pool)
                    .await?;
            return Ok(n as u64);
        }
        let done = sqlx::query("DELETE FROM shared_reply_state WHERE shared_conv LIKE $1")
            .bind(&prefix)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    /// The reply claims of team conversation `tag`.
    pub async fn reply_state(&self, tag: &str) -> Result<ReplyState, sqlx::Error> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT state FROM shared_reply_state WHERE shared_conv = $1")
                .bind(tag)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row
            .and_then(|(s,)| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    }

    /// Decide `user`'s claim on the reply with the row locked, and store it
    /// when it wins.
    pub async fn claim_reply(
        &self,
        tag: &str,
        user: &str,
        seen_seq: u64,
        force: bool,
        at: i64,
    ) -> Result<Claim, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO shared_reply_state (shared_conv, state) VALUES ($1, '{\"seq\":0}')
             ON CONFLICT (shared_conv) DO NOTHING",
        )
        .bind(tag)
        .execute(&mut *tx)
        .await?;
        let (state,): (String,) = sqlx::query_as(
            "SELECT state FROM shared_reply_state WHERE shared_conv = $1 FOR UPDATE",
        )
        .bind(tag)
        .fetch_one(&mut *tx)
        .await?;
        let current: ReplyState = serde_json::from_str(&state).unwrap_or_default();
        let decided = shared_reply::claim(&current, user, seen_seq, force, at);
        if let Claim::Won { next } = &decided {
            write_state(&mut tx, tag, next).await?;
        }
        tx.commit().await?;
        Ok(decided)
    }

    /// Take back claim `claim_id` with the row locked; whether it was still
    /// the last claim.
    pub async fn release_reply(&self, tag: &str, claim_id: &str) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT state FROM shared_reply_state WHERE shared_conv = $1 FOR UPDATE",
        )
        .bind(tag)
        .fetch_optional(&mut *tx)
        .await?;
        let current: ReplyState = row
            .and_then(|(s,)| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let Some(back) = shared_reply::release(&current, claim_id) else {
            tx.rollback().await?;
            return Ok(false);
        };
        write_state(&mut tx, tag, &back).await?;
        tx.commit().await?;
        Ok(true)
    }
}

async fn write_state(
    tx: &mut sqlx::Transaction<'_, crate::pg::BackendDb>,
    tag: &str,
    state: &ReplyState,
) -> Result<(), sqlx::Error> {
    let body = serde_json::to_string(state).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
    sqlx::query("UPDATE shared_reply_state SET state = $2 WHERE shared_conv = $1")
        .bind(tag)
        .bind(body)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

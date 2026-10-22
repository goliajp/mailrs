//! Message mutation: expunge, copy/move, content patching, invite payload.

use crate::pg::BackendPool;

use crate::pg::PgMailboxStore;

impl PgMailboxStore {
    /// Attach an iTIP invite payload (parsed by `mailrs::ical` upstream) to
    /// a previously-stored message. Idempotent: rerunning with new content
    /// overwrites. The caller (server inbound pipeline, MRS-4) extracts
    /// the `text/calendar` MIME part, parses it, and serialises the result
    /// to JSON before passing it here.
    ///
    /// Returns the `messages.id` of the updated row when the (user,
    /// folder, uid) tuple matches an existing message, `None` otherwise
    /// (e.g. when the message was moved or deleted between insertion and
    /// the post-store hook).
    pub async fn update_invite_payload(
        &self,
        user: &str,
        mailbox_name: &str,
        uid: u32,
        invite_payload: &serde_json::Value,
        invite_method: &str,
    ) -> Result<Option<i64>, sqlx::Error> {
        let row: Option<(i64,)> = sqlx::query_as(
            "UPDATE messages
             SET invite_payload = $1, invite_method = $2
             FROM mailboxes
             WHERE messages.mailbox_id = mailboxes.id
               AND mailboxes.user_address = $3
               AND mailboxes.name = $4
               AND messages.uid = $5
             RETURNING messages.id",
        )
        .bind(invite_payload)
        .bind(invite_method)
        .bind(user)
        .bind(mailbox_name)
        .bind(uid as i32)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| id))
    }

    /// Delete those of `uids` that carry `\Deleted`, and return them. A
    /// message flagged after the caller listed the mailbox stays.
    pub async fn expunge_uids(
        &self,
        mailbox_id: i64,
        uids: &[u32],
    ) -> Result<Vec<u32>, sqlx::Error> {
        let mut gone = Vec::with_capacity(uids.len());
        for &uid in uids {
            let deleted = sqlx::query(
                "DELETE FROM messages WHERE mailbox_id = $1 AND uid = $2 AND (flags & $3) != 0",
            )
            .bind(mailbox_id)
            .bind(uid as i32)
            .bind(crate::types::FLAG_DELETED as i32)
            .execute(&self.pool)
            .await?;
            if deleted.rows_affected() > 0 {
                gone.push(uid);
            }
        }
        Ok(gone)
    }

    /// delete messages with \Deleted flag, return expunged UIDs
    pub async fn expunge(&self, mailbox_id: i64) -> Result<Vec<u32>, sqlx::Error> {
        let uid_rows = sqlx::query_as::<_, (i32,)>(
            "SELECT uid FROM messages WHERE mailbox_id = $1 AND (flags & $2) != 0",
        )
        .bind(mailbox_id)
        .bind(crate::types::FLAG_DELETED as i32)
        .fetch_all(&self.pool)
        .await?;

        let uids: Vec<u32> = uid_rows.into_iter().map(|r| r.0 as u32).collect();

        sqlx::query("DELETE FROM messages WHERE mailbox_id = $1 AND (flags & $2) != 0")
            .bind(mailbox_id)
            .bind(crate::types::FLAG_DELETED as i32)
            .execute(&self.pool)
            .await?;

        Ok(uids)
    }

    /// Copy a message's row to another mailbox; returns the new uid, `None`
    /// when the source is gone. `body` is the copy's own `maildir_id`;
    /// `None` keeps the source's.
    pub async fn copy_message(
        &self,
        user: &str,
        src_mailbox_id: i64,
        uid: u32,
        dst_mailbox_name: &str,
        body: Option<&str>,
    ) -> Result<Option<u32>, sqlx::Error> {
        copy_message_inner(
            &self.pool,
            user,
            src_mailbox_id,
            uid,
            dst_mailbox_name,
            body,
        )
        .await
    }

    /// [`Self::copy_message`], then the source row deleted.
    pub async fn move_message(
        &self,
        user: &str,
        src_mailbox_id: i64,
        uid: u32,
        dst_mailbox_name: &str,
        body: Option<&str>,
    ) -> Result<Option<u32>, sqlx::Error> {
        let new_uid = copy_message_inner(
            &self.pool,
            user,
            src_mailbox_id,
            uid,
            dst_mailbox_name,
            body,
        )
        .await?;
        if new_uid.is_some() {
            sqlx::query("DELETE FROM messages WHERE mailbox_id = $1 AND uid = $2")
                .bind(src_mailbox_id)
                .bind(uid as i32)
                .execute(&self.pool)
                .await?;
        }
        Ok(new_uid)
    }

    /// update message content fields after deep cleaning
    // 7 of the args are Option<&str> columns being patched; positional is
    // the cleanest spelling — wrapping in a struct of `Option<&str>` adds
    // no clarity over the column order they already mirror.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_message_content(
        &self,
        message_id: i64,
        text_body: Option<&str>,
        html_body: Option<&str>,
        clean_text: Option<&str>,
        new_content: Option<&str>,
        is_bulk_sender: bool,
        has_tracking_pixel: bool,
        importance_level: &str,
        importance_score: f32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE messages SET
               text_body = COALESCE($2, text_body),
               html_body = COALESCE($3, html_body),
               clean_text = COALESCE($4, clean_text),
               new_content = COALESCE($5, new_content),
               is_bulk_sender = $6,
               has_tracking_pixel = $7,
               importance_level = $8,
               importance_score = $9
             WHERE id = $1",
        )
        .bind(message_id)
        .bind(text_body)
        .bind(html_body)
        .bind(clean_text)
        .bind(new_content)
        .bind(is_bulk_sender)
        .bind(has_tracking_pixel)
        .bind(importance_level)
        .bind(importance_score)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

/// copy message logic extracted as a free function so both copy_message and move_message can call it
async fn copy_message_inner(
    pool: &BackendPool,
    user: &str,
    src_mailbox_id: i64,
    uid: u32,
    dst_mailbox_name: &str,
    body: Option<&str>,
) -> Result<Option<u32>, sqlx::Error> {
    // read source message (including flags to preserve on copy)
    let src = sqlx::query_as::<_, (String, String, String, String, i32, i64, i64, i32, String, String, String)>(
        "SELECT maildir_id, sender, recipients, subject, size, date_epoch, internal_date, flags, message_id, in_reply_to, thread_id
         FROM messages WHERE mailbox_id = $1 AND uid = $2",
    )
    .bind(src_mailbox_id)
    .bind(uid as i32)
    .fetch_optional(pool)
    .await?;

    let src = match src {
        Some(s) => s,
        None => return Ok(None),
    };

    let mut tx = pool.begin().await?;

    let r = super::uids::reserve(&mut tx, user, dst_mailbox_name, 1).await?;
    let (dst_id, new_uid, new_modseq) = (r.mailbox_id, r.first_uid, r.base_modseq + 1);

    sqlx::query(
        "INSERT INTO messages (mailbox_id, uid, maildir_id, sender, recipients, subject, size, date_epoch, internal_date, flags, message_id, in_reply_to, thread_id, modseq)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(dst_id)
    .bind(new_uid)
    .bind(body.unwrap_or(&src.0))   // maildir_id
    .bind(&src.1)   // sender
    .bind(&src.2)   // recipients
    .bind(&src.3)   // subject
    .bind(src.4)    // size
    .bind(src.5)    // date_epoch
    .bind(src.6)    // internal_date
    .bind(src.7)    // flags
    .bind(&src.8)   // message_id
    .bind(&src.9)   // in_reply_to
    .bind(&src.10)  // thread_id
    .bind(new_modseq)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Some(new_uid as u32))
}

//! The maildir-driven threading backfill: give every message with no
//! thread id one, from the Message-ID and In-Reply-To in its file.

use crate::pg::PgMailboxStore;
use crate::pg::helpers::read_raw_from_maildir;
use crate::threading;

impl PgMailboxStore {
    /// backfill threading data for messages that have empty thread_id
    /// reads raw bytes from maildir to extract Message-ID/In-Reply-To
    pub async fn backfill_threading(&self, maildir_root: &str) -> u32 {
        // find all messages missing thread_id
        let entries = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT m.id, m.maildir_id, mb.user_address
             FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE m.thread_id = ''",
        )
        .fetch_all(&self.pool)
        .await;

        let entries = match entries {
            Ok(e) => e,
            Err(_) => return 0,
        };

        let mut count = 0u32;
        for (id, maildir_id, user) in &entries {
            let raw = read_raw_from_maildir(maildir_root, user, maildir_id);
            let Some(data) = raw else { continue };

            let msg_id = threading::extract_message_id(&data);
            if msg_id.is_empty() {
                continue;
            }
            let in_reply_to = threading::extract_in_reply_to(&data);

            // look up parent thread_id
            let parent_tid: Option<String> = if !in_reply_to.is_empty() {
                sqlx::query_as::<_, (String,)>(
                    "SELECT m.thread_id FROM messages m
                     JOIN mailboxes mb ON m.mailbox_id = mb.id
                     WHERE mb.user_address = $1 AND m.message_id = $2 AND m.thread_id != ''
                     LIMIT 1",
                )
                .bind(user)
                .bind(&in_reply_to)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten()
                .map(|r| r.0)
            } else {
                None
            };

            let resolved =
                threading::resolve_thread_id(&msg_id, &in_reply_to, |_| parent_tid.clone());
            // Gmail subject rule — same gate the live index path applies.
            let subject = crate::pg::helpers::extract_header_value(&data, "Subject");
            let thread_id = self
                .apply_subject_gate(user, &msg_id, &subject, parent_tid.clone(), resolved)
                .await;

            let _ = sqlx::query(
                "UPDATE messages SET message_id = $1, in_reply_to = $2, thread_id = $3 WHERE id = $4",
            )
            .bind(&msg_id)
            .bind(&in_reply_to)
            .bind(&thread_id)
            .bind(id)
            .execute(&self.pool)
            .await;

            count += 1;
        }
        count
    }
}

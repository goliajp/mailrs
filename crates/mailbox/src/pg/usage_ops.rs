use crate::pg::PgMailboxStore;

impl PgMailboxStore {
    /// Sum every message's `size` for the given user, in bytes. Used for
    /// per-user quota reporting; returns 0 on query error.
    pub async fn user_storage_usage(&self, user: &str) -> u64 {
        let row: Result<(i64,), _> = sqlx::query_as(
            "SELECT COALESCE(SUM(m.size), 0) FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id WHERE mb.user_address = $1",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await;

        row.map(|r| r.0 as u64).unwrap_or(0)
    }
}

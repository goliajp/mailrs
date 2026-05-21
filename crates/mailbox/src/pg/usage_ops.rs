use crate::pg::PgMailboxStore;

impl PgMailboxStore {
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

use crate::pg::PgMailboxStore;

impl PgMailboxStore {
    /// get concatenated extracted text from all attachments of a message
    pub async fn get_attachment_texts(&self, message_id: i64) -> Result<String, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT COALESCE(extracted_text, '')
             FROM attachment_content
             WHERE message_id = $1 AND attachment_index >= 0 AND extracted_text != ''
             ORDER BY attachment_index",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;

        let combined: String = rows
            .into_iter()
            .map(|r| r.0)
            .collect::<Vec<_>>()
            .join("\n---\n");

        Ok(combined)
    }
}

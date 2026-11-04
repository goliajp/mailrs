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

    /// The text read out of each attachment of the message with this
    /// `Message-ID` (its first copy), in attachment order.
    pub async fn attachment_texts_of(&self, message_id: &str) -> Result<Vec<String>, sqlx::Error> {
        let id: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM messages WHERE message_id = $1 ORDER BY id LIMIT 1")
                .bind(message_id)
                .fetch_optional(&self.pool)
                .await?;
        let Some((id,)) = id else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT COALESCE(extracted_text, '')
             FROM attachment_content
             WHERE message_id = $1 AND attachment_index >= 0 AND extracted_text != ''
             ORDER BY attachment_index",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}

use crate::pg::PgMailboxStore;

/// Input to [`PgMailboxStore::upsert_email_analysis`].
///
/// All fields are required by the underlying `email_analysis` table; the
/// struct exists to keep the call sites readable (the previous signature
/// had 16 positional args). Build it with field syntax — there's no
/// `Default` because every field is semantically required.
#[derive(Debug, Clone)]
pub struct EmailAnalysisInput<'a> {
    /// `messages.id` of the message this analysis describes.
    pub message_id: i64,
    /// Coarse category label (e.g. `personal`, `work`, `bulk`, `spam`, `scam`).
    pub category: &'a str,
    /// Integer risk score on a `0..=100` scale.
    pub risk_score: i16,
    /// Short human-readable justification for `risk_score`.
    pub risk_reason: &'a str,
    /// One- or two-sentence summary of the message body.
    pub summary: &'a str,
    /// JSON array of named entities (people) extracted from the message.
    pub people: &'a serde_json::Value,
    /// JSON array of date references extracted from the message.
    pub dates: &'a serde_json::Value,
    /// JSON array of monetary amounts extracted from the message.
    pub amounts: &'a serde_json::Value,
    /// JSON array of action items / tasks the message asks of the recipient.
    pub action_items: &'a serde_json::Value,
    /// Optional dense vector for pgvector semantic search.
    pub embedding: Option<&'a [f32]>,
    /// Tag identifying the model that produced this analysis (used to
    /// detect rows that need re-analysis when the model is upgraded).
    pub model_version: &'a str,
    /// Plain-text projection of the message body (HTML stripped, etc.)
    /// cached for downstream consumers.
    pub clean_text: &'a str,
    /// True when the analyser thinks the message asks the recipient to do
    /// something (used to boost importance score).
    pub requires_action: bool,
    /// Inferred sender intent (e.g. `inform`, `request`, `confirm`).
    pub sender_intent: &'a str,
    /// Optional ISO-8601 deadline string parsed from the message body.
    pub action_deadline: Option<&'a str>,
}

impl PgMailboxStore {
    /// get analysis result for a message
    pub async fn get_email_analysis(&self, message_id: i64) -> Result<Option<crate::types::EmailAnalysisRow>, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64, String, i16, String, String, serde_json::Value, serde_json::Value, serde_json::Value, serde_json::Value, String, String, bool, String, Option<String>)>(
            "SELECT message_id, category, risk_score, risk_reason, summary, people, dates, amounts, action_items, model_version,
                    COALESCE(clean_text, ''), COALESCE(requires_action, false), COALESCE(sender_intent, 'inform'),
                    action_deadline::text
             FROM email_analysis WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| crate::types::EmailAnalysisRow {
            message_id: r.0,
            category: r.1,
            risk_score: r.2,
            risk_reason: r.3,
            summary: r.4,
            people: r.5,
            dates: r.6,
            amounts: r.7,
            action_items: r.8,
            model_version: r.9,
            clean_text: r.10,
            requires_action: r.11,
            sender_intent: r.12,
            action_deadline: r.13,
        }))
    }

    /// Upsert AI analysis result for a message. Takes [`EmailAnalysisInput`]
    /// to keep call sites readable — the underlying table has 16 columns.
    pub async fn upsert_email_analysis(
        &self,
        input: &EmailAnalysisInput<'_>,
    ) -> Result<(), sqlx::Error> {
        // format embedding as pgvector text literal
        let embedding_str = input.embedding.map(|v| {
            let nums: Vec<String> = v.iter().map(|f| f.to_string()).collect();
            format!("[{}]", nums.join(","))
        });

        sqlx::query(
            "INSERT INTO email_analysis (message_id, category, risk_score, risk_reason, summary, people, dates, amounts, action_items, embedding, model_version, clean_text, requires_action, sender_intent, action_deadline, analyzed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::vector, $11, $12, $13, $14, $15::timestamptz, now())
             ON CONFLICT (message_id) DO UPDATE SET
               category = EXCLUDED.category,
               risk_score = EXCLUDED.risk_score,
               risk_reason = EXCLUDED.risk_reason,
               summary = EXCLUDED.summary,
               people = EXCLUDED.people,
               dates = EXCLUDED.dates,
               amounts = EXCLUDED.amounts,
               action_items = EXCLUDED.action_items,
               embedding = EXCLUDED.embedding,
               model_version = EXCLUDED.model_version,
               clean_text = EXCLUDED.clean_text,
               requires_action = EXCLUDED.requires_action,
               sender_intent = EXCLUDED.sender_intent,
               action_deadline = EXCLUDED.action_deadline,
               analyzed_at = now()",
        )
        .bind(input.message_id)
        .bind(input.category)
        .bind(input.risk_score)
        .bind(input.risk_reason)
        .bind(input.summary)
        .bind(input.people)
        .bind(input.dates)
        .bind(input.amounts)
        .bind(input.action_items)
        .bind(embedding_str.as_deref())
        .bind(input.model_version)
        .bind(input.clean_text)
        .bind(input.requires_action)
        .bind(input.sender_intent)
        .bind(input.action_deadline)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// list message IDs that haven't been analyzed yet or need reanalysis (outdated model_version)
    pub async fn list_unanalyzed_message_ids(&self, limit: i64, current_version: &str) -> Result<Vec<(i64, String, String, String, String)>, sqlx::Error> {
        // returns (message_id, user_address, maildir_id, sender, subject)
        let rows = sqlx::query_as::<_, (i64, String, String, String, String)>(
            "SELECT m.id, mb.user_address, m.maildir_id, m.sender, m.subject
             FROM messages m
             JOIN mailboxes mb ON m.mailbox_id = mb.id
             LEFT JOIN email_analysis ea ON m.id = ea.message_id
             WHERE ea.message_id IS NULL OR ea.model_version != $2
             ORDER BY m.id DESC
             LIMIT $1",
        )
        .bind(limit)
        .bind(current_version)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// count total messages needing analysis (unanalyzed + outdated version)
    pub async fn count_unanalyzed_messages(&self, current_version: &str) -> Result<i64, sqlx::Error> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*)
             FROM messages m
             LEFT JOIN email_analysis ea ON m.id = ea.message_id
             WHERE ea.message_id IS NULL OR ea.model_version != $1",
        )
        .bind(current_version)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// boost importance score when AI detects action items
    pub async fn boost_importance_for_action(&self, message_id: i64) -> Result<(), sqlx::Error> {
        // add 0.2 to importance_score and re-evaluate level
        sqlx::query(
            "UPDATE messages SET
               importance_score = LEAST(1.0, importance_score + 0.2),
               importance_level = CASE
                 WHEN LEAST(1.0, importance_score + 0.2) >= 0.8 THEN 'critical'
                 WHEN LEAST(1.0, importance_score + 0.2) >= 0.5 THEN 'important'
                 WHEN LEAST(1.0, importance_score + 0.2) >= 0.2 THEN 'normal'
                 WHEN LEAST(1.0, importance_score + 0.2) >= 0.0 THEN 'low'
                 ELSE 'noise'
               END
             WHERE id = $1",
        )
        .bind(message_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

use crate::pg::PgMailboxStore;

/// contact info for importance scoring
pub struct ContactInfo {
    /// True when the user has sent at least one message to this contact
    /// (i.e. the relationship is two-way).
    pub is_mutual: bool,
    /// True when the contact is recognised as a mailing-list sender.
    pub is_mailing_list: bool,
    /// True when the user has explicitly marked the contact as VIP.
    pub is_vip: bool,
    /// True when the user has explicitly blocked the contact.
    pub is_blocked: bool,
    /// Manual importance adjustment in `[-1.0, 1.0]` accumulated from
    /// per-sender feedback (mark-important, mark-spam, etc.).
    pub importance_bias: f32,
    /// Lifetime count of messages received from this contact.
    pub received_count: i32,
    /// Lifetime count of messages sent to this contact.
    pub sent_count: i32,
}

impl PgMailboxStore {
    /// upsert a contact on inbound email (received from sender)
    pub async fn upsert_contact_inbound(
        &self,
        user: &str,
        sender_email: &str,
        display_name: &str,
        is_mailing_list: bool,
        is_automated: bool,
    ) -> Result<(), sqlx::Error> {
        let email = normalize_email(sender_email);
        sqlx::query(
            "INSERT INTO email_contacts (user_address, email, display_name, first_seen, last_seen, received_count, is_mailing_list, is_automated)
             VALUES ($1, $2, $3, now(), now(), 1, $4, $5)
             ON CONFLICT (user_address, email) DO UPDATE SET
               display_name = CASE WHEN EXCLUDED.display_name != '' THEN EXCLUDED.display_name ELSE email_contacts.display_name END,
               last_seen = now(),
               received_count = email_contacts.received_count + 1,
               is_mailing_list = email_contacts.is_mailing_list OR EXCLUDED.is_mailing_list,
               is_automated = email_contacts.is_automated OR EXCLUDED.is_automated",
        )
        .bind(user)
        .bind(&email)
        .bind(display_name)
        .bind(is_mailing_list)
        .bind(is_automated)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// upsert a contact on outbound email (sent to recipient)
    pub async fn upsert_contact_outbound(
        &self,
        user: &str,
        recipient_email: &str,
        display_name: &str,
    ) -> Result<(), sqlx::Error> {
        let email = normalize_email(recipient_email);
        sqlx::query(
            "INSERT INTO email_contacts (user_address, email, display_name, first_seen, last_seen, sent_count, is_mutual)
             VALUES ($1, $2, $3, now(), now(), 1, true)
             ON CONFLICT (user_address, email) DO UPDATE SET
               display_name = CASE WHEN EXCLUDED.display_name != '' THEN EXCLUDED.display_name ELSE email_contacts.display_name END,
               last_seen = now(),
               sent_count = email_contacts.sent_count + 1,
               is_mutual = true",
        )
        .bind(user)
        .bind(&email)
        .bind(display_name)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// mark contact as mutual (when user replies to a sender)
    pub async fn mark_contact_mutual(&self, user: &str, email: &str) -> Result<(), sqlx::Error> {
        let email = normalize_email(email);
        sqlx::query(
            "UPDATE email_contacts SET is_mutual = true, reply_count = reply_count + 1
             WHERE user_address = $1 AND email = $2",
        )
        .bind(user)
        .bind(&email)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// get contact info for importance scoring
    pub async fn get_contact_for_scoring(
        &self,
        user: &str,
        sender_email: &str,
    ) -> Result<Option<ContactInfo>, sqlx::Error> {
        let email = normalize_email(sender_email);
        let row = sqlx::query_as::<_, (bool, bool, bool, bool, f32, i32, i32)>(
            "SELECT is_mutual, is_mailing_list, is_vip, is_blocked, importance_bias, received_count, sent_count
             FROM email_contacts WHERE user_address = $1 AND email = $2",
        )
        .bind(user)
        .bind(&email)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| ContactInfo {
            is_mutual: r.0,
            is_mailing_list: r.1,
            is_vip: r.2,
            is_blocked: r.3,
            importance_bias: r.4,
            received_count: r.5,
            sent_count: r.6,
        }))
    }

    /// check if user has sent email to this address (for is_reply_to_my_email detection)
    pub async fn has_sent_to(
        &self,
        user: &str,
        recipient_email: &str,
    ) -> Result<bool, sqlx::Error> {
        let email = normalize_email(recipient_email);
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM email_contacts
             WHERE user_address = $1 AND email = $2 AND sent_count > 0",
        )
        .bind(user)
        .bind(&email)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0 > 0)
    }

    /// record user feedback on a sender (for learning)
    pub async fn record_sender_feedback(
        &self,
        user: &str,
        sender_email: &str,
        action: &str,
    ) -> Result<(), sqlx::Error> {
        let email = normalize_email(sender_email);
        sqlx::query(
            "INSERT INTO sender_feedback (user_address, sender_email, action) VALUES ($1, $2, $3)",
        )
        .bind(user)
        .bind(&email)
        .bind(action)
        .execute(&self.pool)
        .await?;

        // update contact importance_bias based on action
        let bias_delta: f32 = match action {
            "mark_important" => 0.2,
            "mark_vip" => 0.4,
            "mark_spam" | "block" => -0.5,
            "unblock" => 0.5,
            "archive" => -0.05,
            _ => 0.0,
        };

        if bias_delta.abs() > f32::EPSILON {
            sqlx::query(
                "UPDATE email_contacts SET importance_bias = LEAST(1.0, GREATEST(-1.0, importance_bias + $3))
                 WHERE user_address = $1 AND email = $2",
            )
            .bind(user)
            .bind(&email)
            .bind(bias_delta)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }
}

/// normalize email address: lowercase, remove +tags
fn normalize_email(email: &str) -> String {
    let email = email.trim().to_lowercase();
    // extract bare email from "Display Name <email@domain>" format
    let email = if let Some(start) = email.find('<') {
        if let Some(end) = email.find('>') {
            email[start + 1..end].to_string()
        } else {
            email
        }
    } else {
        email
    };

    // remove + tags (e.g., user+tag@domain -> user@domain)
    if let Some((local, domain)) = email.split_once('@') {
        let local = if let Some((base, _)) = local.split_once('+') {
            base
        } else {
            local
        };
        format!("{local}@{domain}")
    } else {
        email
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_email_basic() {
        assert_eq!(normalize_email("Alice@Example.COM"), "alice@example.com");
    }

    #[test]
    fn normalize_email_with_display_name() {
        assert_eq!(
            normalize_email("Alice <alice@example.com>"),
            "alice@example.com"
        );
        assert_eq!(normalize_email("\"Bob\" <BOB@Test.COM>"), "bob@test.com");
    }

    #[test]
    fn normalize_email_removes_plus_tag() {
        assert_eq!(normalize_email("user+tag@example.com"), "user@example.com");
        assert_eq!(
            normalize_email("alice+newsletter@test.com"),
            "alice@test.com"
        );
    }

    #[test]
    fn normalize_email_no_plus_tag() {
        assert_eq!(normalize_email("alice@example.com"), "alice@example.com");
    }

    #[test]
    fn normalize_email_trims_whitespace() {
        assert_eq!(
            normalize_email("  alice@example.com  "),
            "alice@example.com"
        );
    }

    #[test]
    fn normalize_email_bare_string() {
        assert_eq!(normalize_email("notanemail"), "notanemail");
    }
}

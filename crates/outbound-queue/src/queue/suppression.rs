//! Suppression list (hard-bounce blocklist).

#[cfg(feature = "pg")]
use crate::BackendPool;

/// check if a recipient address is in the suppression list (hard bounce)
#[cfg(feature = "pg")]
pub async fn is_suppressed(pool: &BackendPool, email: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM suppression_list WHERE email = $1)")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap_or(false)
}

/// add a recipient to the suppression list after a hard bounce
#[cfg(feature = "pg")]
pub async fn add_suppression(
    pool: &BackendPool,
    email: &str,
    reason: &str,
    smtp_code: Option<i32>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO suppression_list (email, reason, bounce_type, smtp_code) \
         VALUES ($1, $2, 'hard', $3) \
         ON CONFLICT (email) DO UPDATE SET reason = $2, smtp_code = $3, created_at = NOW()",
    )
    .bind(email)
    .bind(reason)
    .bind(smtp_code)
    .execute(pool)
    .await?;
    Ok(())
}

/// remove an address from the suppression list (admin override)
#[cfg(feature = "pg")]
pub async fn remove_suppression(pool: &BackendPool, email: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM suppression_list WHERE email = $1")
        .bind(email)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// list all suppressed addresses
#[cfg(feature = "pg")]
pub async fn list_suppressions(
    pool: &BackendPool,
    limit: i64,
) -> Result<Vec<(String, String, Option<i32>, i64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT email, reason, smtp_code, EXTRACT(EPOCH FROM created_at)::BIGINT \
         FROM suppression_list ORDER BY created_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// detect if an SMTP error is a permanent/hard bounce (5xx)
pub fn is_hard_bounce(error: &str) -> bool {
    let trimmed = error.trim();
    trimmed.starts_with('5') || trimmed.starts_with("5.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_bounce_detects_5xx() {
        assert!(is_hard_bounce("550 mailbox unavailable"));
        assert!(is_hard_bounce("552 message too large"));
        assert!(is_hard_bounce("5.7.1 policy reject"));
    }

    #[test]
    fn hard_bounce_rejects_4xx_and_other() {
        assert!(!is_hard_bounce("450 try again"));
        assert!(!is_hard_bounce("421 service unavailable"));
        assert!(!is_hard_bounce("250 ok"));
        assert!(!is_hard_bounce(""));
        assert!(!is_hard_bounce("random text"));
    }

    #[test]
    fn hard_bounce_trims_leading_whitespace() {
        assert!(is_hard_bounce("  550 spaces"));
        assert!(!is_hard_bounce("  450 spaces"));
    }
}

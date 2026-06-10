//! Vacation auto-reply dedup state (RFC 5230 §4.6).
//!
//! Correctness over speed: no Kevy / process cache here — the dedup
//! check must hit PG so concurrent inbound deliveries can't both slip
//! a reply through. The window is enforced at query time by comparing
//! `last_sent_at` against `now`.

use super::{DomainStore, Result};

impl DomainStore {
    /// Whether a vacation reply may be sent to `sender` now: `true`
    /// when there is no prior record for this (recipient, sender,
    /// handle) triple, or the dedup window (`period_secs`) has elapsed
    /// since the last reply.
    pub async fn should_send_vacation_reply(
        &self,
        recipient: &str,
        sender: &str,
        handle: &str,
        period_secs: u64,
    ) -> Result<bool> {
        let pool = self.pg()?;
        let now = chrono::Utc::now().timestamp();
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT last_sent_at FROM vacation_dedup \
             WHERE recipient = $1 AND sender = $2 AND handle = $3",
        )
        .bind(recipient)
        .bind(sender)
        .bind(handle)
        .fetch_optional(pool)
        .await?;
        Ok(match row {
            Some((last,)) => now - last >= period_secs as i64,
            None => true,
        })
    }

    /// Record that a vacation reply was just sent for this triple,
    /// upserting `last_sent_at`.
    pub async fn record_vacation_reply(
        &self,
        recipient: &str,
        sender: &str,
        handle: &str,
        now: i64,
    ) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query(
            "INSERT INTO vacation_dedup (recipient, sender, handle, last_sent_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (recipient, sender, handle) \
             DO UPDATE SET last_sent_at = EXCLUDED.last_sent_at",
        )
        .bind(recipient)
        .bind(sender)
        .bind(handle)
        .bind(now)
        .execute(pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::HealthState;

    // real-PG test (no mocks). Gated on MAILRS_PG_URL so the default
    // `cargo test` skips it; run with:
    //   MAILRS_PG_URL=postgres://mailrs:mailrs@127.0.0.1/mailrs \
    //     cargo test -p mailrs-server -- --ignored vacation
    #[tokio::test]
    #[ignore = "requires MAILRS_PG_URL"]
    async fn dedup_window_lifecycle() {
        let url = std::env::var("MAILRS_PG_URL").expect("MAILRS_PG_URL required");
        let pool = crate::pg::create_pool(&url).await.expect("DB pool");
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS vacation_dedup (\
             recipient TEXT NOT NULL, sender TEXT NOT NULL, handle TEXT NOT NULL, \
             last_sent_at BIGINT NOT NULL, PRIMARY KEY (recipient, sender, handle))",
        )
        .execute(&pool)
        .await
        .expect("create table");

        let store = DomainStore::new(Some(pool.clone()), None, HealthState::new());
        let rcpt = "vac-test-rcpt@example.com";
        let sender = "vac-test-sender@example.com";
        let handle = "h1";
        let win = 3600u64;

        // clean slate
        sqlx::query("DELETE FROM vacation_dedup WHERE recipient = $1")
            .bind(rcpt)
            .execute(&pool)
            .await
            .unwrap();

        let now = chrono::Utc::now().timestamp();

        // first time → send
        assert!(
            store
                .should_send_vacation_reply(rcpt, sender, handle, win)
                .await
                .unwrap()
        );
        store
            .record_vacation_reply(rcpt, sender, handle, now)
            .await
            .unwrap();

        // within window → suppressed
        assert!(
            !store
                .should_send_vacation_reply(rcpt, sender, handle, win)
                .await
                .unwrap()
        );

        // distinct sender / handle → independent
        assert!(
            store
                .should_send_vacation_reply(rcpt, "other@example.com", handle, win)
                .await
                .unwrap()
        );
        assert!(
            store
                .should_send_vacation_reply(rcpt, sender, "h2", win)
                .await
                .unwrap()
        );

        // past window (last reply 2h ago, window 1h) → send again
        store
            .record_vacation_reply(rcpt, sender, handle, now - 7200)
            .await
            .unwrap();
        assert!(
            store
                .should_send_vacation_reply(rcpt, sender, handle, win)
                .await
                .unwrap()
        );

        // cleanup
        sqlx::query("DELETE FROM vacation_dedup WHERE recipient = $1")
            .bind(rcpt)
            .execute(&pool)
            .await
            .unwrap();
    }
}

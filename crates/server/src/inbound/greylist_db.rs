use redis::AsyncCommands;

use super::greylisting::{evaluate_triplet, GreylistConfig, GreylistDecision};

pub struct GreylistDb {
    valkey: redis::aio::ConnectionManager,
    pg: Option<sqlx::PgPool>,
}

impl GreylistDb {
    pub fn new(valkey: redis::aio::ConnectionManager) -> Self {
        Self { valkey, pg: None }
    }

    pub fn with_pg(mut self, pool: sqlx::PgPool) -> Self {
        self.pg = Some(pool);
        self
    }

    pub async fn check(&self, key: &str, now: u64, config: &GreylistConfig) -> GreylistDecision {
        let mut conn = self.valkey.clone();
        let vk_key = format!("gl:{key}");

        let first_seen: Option<u64> = conn.get(&vk_key).await.ok().flatten();

        let decision = evaluate_triplet(first_seen, now, config);

        match decision {
            GreylistDecision::Defer => {
                // first time — set with TTL = pass_ttl
                let _: Result<(), _> = conn.set_ex(&vk_key, now, config.pass_ttl_secs).await;
            }
            GreylistDecision::TooEarly | GreylistDecision::Accept => {
                // update TTL to keep entry alive
                let _: Result<(), _> = conn.expire(&vk_key, config.pass_ttl_secs as i64).await;
            }
        }

        // cold backup to PG (best effort)
        if let Some(ref pool) = self.pg {
            let now_i64 = now as i64;
            let _ = sqlx::query(
                "INSERT INTO greylist_triplets (key, first_seen, last_seen)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (key) DO UPDATE SET last_seen = $3",
            )
            .bind(key)
            .bind(first_seen.unwrap_or(now) as i64)
            .bind(now_i64)
            .execute(pool)
            .await;
        }

        decision
    }
}

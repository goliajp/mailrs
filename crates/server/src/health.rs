use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

pub const PG_UP: u8 = 0x01;
pub const VALKEY_UP: u8 = 0x02;

#[derive(Clone)]
pub struct HealthState {
    flags: Arc<AtomicU8>,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            flags: Arc::new(AtomicU8::new(0)),
        }
    }

    pub fn set_pg(&self, up: bool) {
        if up {
            self.flags.fetch_or(PG_UP, Ordering::Relaxed);
        } else {
            self.flags.fetch_and(!PG_UP, Ordering::Relaxed);
        }
    }

    pub fn set_valkey(&self, up: bool) {
        if up {
            self.flags.fetch_or(VALKEY_UP, Ordering::Relaxed);
        } else {
            self.flags.fetch_and(!VALKEY_UP, Ordering::Relaxed);
        }
    }

    pub fn pg_up(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & PG_UP != 0
    }

    pub fn valkey_up(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & VALKEY_UP != 0
    }

    pub fn level(&self) -> u8 {
        let f = self.flags.load(Ordering::Relaxed);
        match (f & PG_UP != 0, f & VALKEY_UP != 0) {
            (true, true) => 0,
            (false, true) => 1,
            (true, false) => 2,
            (false, false) => 3,
        }
    }
}

pub fn spawn_health_checker(
    pg: sqlx::PgPool,
    valkey: redis::aio::ConnectionManager,
    state: HealthState,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;

            // ping PG
            let pg_ok = sqlx::query("SELECT 1")
                .execute(&pg)
                .await
                .is_ok();
            state.set_pg(pg_ok);

            // ping Valkey
            let valkey_ok = {
                let mut conn = valkey.clone();
                redis::cmd("PING")
                    .query_async::<String>(&mut conn)
                    .await
                    .is_ok()
            };
            state.set_valkey(valkey_ok);
        }
    });
}

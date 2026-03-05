use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub const PG_UP: u8 = 0x01;
pub const VALKEY_UP: u8 = 0x02;

#[derive(Clone)]
pub struct HealthState {
    flags: Arc<AtomicU8>,
    started_at: Instant,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            flags: Arc::new(AtomicU8::new(0)),
            started_at: Instant::now(),
        }
    }

    /// server uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
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

    /// returns "healthy", "degraded", or "unhealthy"
    pub fn status_label(&self) -> &'static str {
        match self.level() {
            0 => "healthy",
            1 | 2 => "degraded",
            _ => "unhealthy",
        }
    }

    /// true if the server can accept traffic (PG is up)
    pub fn is_ready(&self) -> bool {
        self.pg_up()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_state_initial() {
        let hs = HealthState::new();
        assert!(!hs.pg_up());
        assert!(!hs.valkey_up());
        assert_eq!(hs.level(), 3);
        assert_eq!(hs.status_label(), "unhealthy");
        assert!(!hs.is_ready());
    }

    #[test]
    fn health_state_pg_only() {
        let hs = HealthState::new();
        hs.set_pg(true);
        assert!(hs.pg_up());
        assert!(!hs.valkey_up());
        assert_eq!(hs.level(), 2);
        assert_eq!(hs.status_label(), "degraded");
        assert!(hs.is_ready());
    }

    #[test]
    fn health_state_valkey_only() {
        let hs = HealthState::new();
        hs.set_valkey(true);
        assert!(!hs.pg_up());
        assert!(hs.valkey_up());
        assert_eq!(hs.level(), 1);
        assert_eq!(hs.status_label(), "degraded");
        assert!(!hs.is_ready());
    }

    #[test]
    fn health_state_both_up() {
        let hs = HealthState::new();
        hs.set_pg(true);
        hs.set_valkey(true);
        assert_eq!(hs.level(), 0);
        assert_eq!(hs.status_label(), "healthy");
        assert!(hs.is_ready());
    }

    #[test]
    fn health_state_toggle() {
        let hs = HealthState::new();
        hs.set_pg(true);
        assert!(hs.pg_up());
        hs.set_pg(false);
        assert!(!hs.pg_up());
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
            let pg_ok = sqlx::query("SELECT 1").execute(&pg).await.is_ok();
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

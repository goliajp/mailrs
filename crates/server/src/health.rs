use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

pub const PG_UP: u8 = 0x01;
pub const KEVY_UP: u8 = 0x02;

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

    pub fn set_kevy(&self, up: bool) {
        if up {
            self.flags.fetch_or(KEVY_UP, Ordering::Relaxed);
        } else {
            self.flags.fetch_and(!KEVY_UP, Ordering::Relaxed);
        }
    }

    pub fn pg_up(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & PG_UP != 0
    }

    pub fn kevy_up(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & KEVY_UP != 0
    }

    pub fn level(&self) -> u8 {
        let f = self.flags.load(Ordering::Relaxed);
        match (f & PG_UP != 0, f & KEVY_UP != 0) {
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
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn health_state_initial() {
        let hs = HealthState::new();
        assert!(!hs.pg_up());
        assert!(!hs.kevy_up());
        assert_eq!(hs.level(), 3);
        assert_eq!(hs.status_label(), "unhealthy");
        assert!(!hs.is_ready());
    }

    #[test]
    fn health_state_pg_only() {
        let hs = HealthState::new();
        hs.set_pg(true);
        assert!(hs.pg_up());
        assert!(!hs.kevy_up());
        assert_eq!(hs.level(), 2);
        assert_eq!(hs.status_label(), "degraded");
        assert!(hs.is_ready());
    }

    #[test]
    fn health_state_kevy_only() {
        let hs = HealthState::new();
        hs.set_kevy(true);
        assert!(!hs.pg_up());
        assert!(hs.kevy_up());
        assert_eq!(hs.level(), 1);
        assert_eq!(hs.status_label(), "degraded");
        assert!(!hs.is_ready());
    }

    #[test]
    fn health_state_both_up() {
        let hs = HealthState::new();
        hs.set_pg(true);
        hs.set_kevy(true);
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
    pg: crate::pg::BackendPool,
    kevy: crate::kevy_store::KevyStore,
    state: HealthState,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;

            // ping PG
            let pg_ok = sqlx::query("SELECT 1").execute(&pg).await.is_ok();
            state.set_pg(pg_ok);

            // exercise the embed store — set + read a probe key. This
            // catches a poisoned mutex or AOF write failure but is O(1)
            // because the store is in-process Arc<Store>.
            let kevy_ok = kevy
                .set(b"_health_probe", b"ok")
                .and_then(|_| kevy.get(b"_health_probe"))
                .is_ok();
            state.set_kevy(kevy_ok);
        }
    });
}

//! Time, behind a handle the state-machine tests can move by hand.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Clone)]
pub enum Clock {
    System,
    /// Unix seconds set by the test.
    Manual(Arc<AtomicI64>),
}

impl Clock {
    pub fn manual(at: i64) -> Self {
        Clock::Manual(Arc::new(AtomicI64::new(at)))
    }

    pub fn now(&self) -> i64 {
        match self {
            Clock::System => mailrs_ops_report::build::now(),
            Clock::Manual(t) => t.load(Ordering::Relaxed),
        }
    }

    /// Move a manual clock forward; a no-op on the system clock.
    pub fn advance(&self, secs: i64) {
        if let Clock::Manual(t) = self {
            t.fetch_add(secs, Ordering::Relaxed);
        }
    }
}

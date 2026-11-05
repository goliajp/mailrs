//! Which binary is running: the release version (injected into the
//! workspace version at image build), the commit (`MAILRS_BUILD_COMMIT`
//! at compile time) and when this process started.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The build a process reports, in its reports and in its health answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub version: String,
    /// `None` for a binary built without `MAILRS_BUILD_COMMIT`.
    pub commit: Option<String>,
    /// Unix seconds this process started.
    pub started_at: i64,
}

static STARTED_AT: OnceLock<i64> = OnceLock::new();

/// Fix this process's start time now. Called first thing in `main`; a
/// process that never calls it gets the time of the first [`build`].
pub fn mark_start() {
    STARTED_AT.get_or_init(now);
}

/// This binary's build and this process's start time.
pub fn build() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        commit: option_env!("MAILRS_BUILD_COMMIT")
            .filter(|c| !c.trim().is_empty())
            .map(str::to_string),
        started_at: *STARTED_AT.get_or_init(now),
    }
}

/// Unix seconds.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_time_does_not_move() {
        mark_start();
        let a = build().started_at;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(build().started_at, a);
        assert!(!build().version.is_empty());
    }
}

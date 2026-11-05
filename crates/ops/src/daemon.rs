//! Running the daemon: listeners, the cycle loop, and its own watchdog.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use crate::app::App;
use crate::clock::Clock;
use crate::config::Config;
use crate::store::Store;

/// How often queued notices are looked at between cycles, so a group is
/// sent soon after its window rather than at the next cycle.
const FLUSH_EVERY: Duration = Duration::from_secs(5);
/// A cycle that has not completed within this many intervals stops the
/// daemon, and the platform starts it again.
const WATCHDOG_INTERVALS: i64 = 5;

pub async fn serve(cfg: Config) -> std::process::ExitCode {
    let store = match Store::open(&cfg.store_path()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ops store {}: {e}", cfg.store_path().display());
            return std::process::ExitCode::from(1);
        }
    };
    let app = match App::open(cfg.clone(), Clock::System, store) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ops daemon: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let tcp = match tokio::net::TcpListener::bind(&cfg.bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind {}: {e}", cfg.bind);
            return std::process::ExitCode::from(1);
        }
    };
    let socket_path = cfg.socket_path();
    // a socket file left by the previous run refuses the bind
    let _ = std::fs::remove_file(&socket_path);
    let unix = match tokio::net::UnixListener::bind(&socket_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind {}: {e}", socket_path.display());
            return std::process::ExitCode::from(1);
        }
    };
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) =
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
        {
            tracing::warn!(error = %e, "ops socket permissions not set");
        }
    }
    tracing::info!(
        bind = %cfg.bind,
        socket = %socket_path.display(),
        interval = cfg.interval_secs,
        heartbeat = cfg.heartbeat_url.is_some(),
        "mailrs-ops running"
    );
    if cfg.heartbeat_url.is_none() {
        tracing::warn!("MAILRS_OPS_HEARTBEAT_URL is not set: nothing outside watches this daemon");
    }
    let (tcp_app, unix_app) = (
        crate::api::tcp_router(app.clone()),
        crate::api::socket_router(app.clone()),
    );
    tokio::spawn(async move { axum::serve(tcp, tcp_app).await });
    tokio::spawn(async move { axum::serve(unix, unix_app).await });
    spawn_watchdog(&app, cfg.interval_secs * WATCHDOG_INTERVALS, |stuck| {
        tracing::error!(
            event = "watchdog_expired",
            stuck_secs = stuck,
            "no cycle completed; exiting"
        );
        std::process::exit(70);
    });
    let flusher = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(FLUSH_EVERY).await;
            flusher.flush().await;
        }
    });
    let mut tick = tokio::time::interval(Duration::from_secs(cfg.interval_secs as u64));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        app.cycle().await;
    }
}

/// Seconds since the last cycle started or finished, once that is longer
/// than `limit`.
pub fn overdue(started: i64, finished: i64, now: i64, limit: i64) -> Option<i64> {
    let last = started.max(finished);
    (now - last > limit).then_some(now - last)
}

/// A plain thread, outside the runtime, so a runtime that stops scheduling
/// cannot stop it too.
pub fn spawn_watchdog(app: &Arc<App>, limit: i64, on_expire: impl Fn(i64) + Send + 'static) {
    watch(
        Arc::new(AtomicRef(app.clone())),
        limit,
        Duration::from_secs(5),
        on_expire,
    );
}

/// The two marks the watchdog reads.
pub trait Marks: Send + Sync + 'static {
    fn marks(&self) -> (i64, i64);
}

struct AtomicRef(Arc<App>);

impl Marks for AtomicRef {
    fn marks(&self) -> (i64, i64) {
        (
            self.0.cycle_started.load(Ordering::Relaxed),
            self.0.cycle_finished.load(Ordering::Relaxed),
        )
    }
}

impl Marks for (AtomicI64, AtomicI64) {
    fn marks(&self) -> (i64, i64) {
        (
            self.0.load(Ordering::Relaxed),
            self.1.load(Ordering::Relaxed),
        )
    }
}

pub fn watch<M: Marks>(
    marks: Arc<M>,
    limit: i64,
    every: Duration,
    on_expire: impl Fn(i64) + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("ops-watchdog".into())
        .spawn(move || {
            loop {
                std::thread::sleep(every);
                let (started, finished) = marks.marks();
                if let Some(stuck) =
                    overdue(started, finished, mailrs_ops_report::build::now(), limit)
                {
                    on_expire(stuck);
                    return;
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "ops watchdog thread not started");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overdue_counts_from_the_later_mark() {
        assert_eq!(overdue(100, 90, 350, 300), None);
        assert_eq!(overdue(100, 90, 401, 300), Some(301));
        // finished after it started: the loop is between cycles
        assert_eq!(overdue(100, 200, 450, 300), None);
    }

    #[test]
    fn a_stuck_cycle_trips_the_watchdog_and_a_moving_one_does_not() {
        let now = mailrs_ops_report::build::now();
        let stuck = Arc::new((AtomicI64::new(now - 10), AtomicI64::new(now - 20)));
        let (tx, rx) = std::sync::mpsc::channel();
        watch(stuck, 5, Duration::from_millis(20), move |s| {
            let _ = tx.send(s);
        });
        let secs = rx.recv_timeout(Duration::from_secs(2)).expect("tripped");
        assert!(secs >= 10, "{secs}");

        let moving = Arc::new((AtomicI64::new(now + 3600), AtomicI64::new(now)));
        let (tx, rx) = std::sync::mpsc::channel();
        watch(moving, 5, Duration::from_millis(20), move |s| {
            let _ = tx.send(s);
        });
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }
}

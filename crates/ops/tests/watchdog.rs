//! The daemon's own watchdog, on the real binary: once a cycle stops
//! finishing, the heartbeat stops with it, and within five intervals the
//! daemon exits (status 70) for the platform to start it again.
//!
//! Needs `--features stall-hook`; no shipped binary has it.

#![cfg(feature = "stall-hook")]

mod common;

use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stuck_cycle_stops_the_heartbeat_and_the_daemon() {
    let (beat, beats) = common::receiver().await;
    let data = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mailrs-ops"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("MAILRS_OPS_DATA_DIR", data.path())
        .env("MAILRS_OPS_BIND", "127.0.0.1:0")
        .env("MAILRS_OPS_INTERVAL_SECS", "1")
        .env("MAILRS_OPS_FASTCORE_URL", "http://127.0.0.1:9")
        .env("MAILRS_OPS_WEBAPI_URL", "http://127.0.0.1:9")
        .env("MAILRS_OPS_HEARTBEAT_URL", &beat)
        .env("MAILRS_OPS_TEST_STALL", "1")
        .env("RUST_LOG", "warn")
        .spawn()
        .expect("start mailrs-ops");
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break Some(s);
        }
        if started.elapsed() > Duration::from_secs(30) {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let Some(status) = status else {
        // only the process this test started
        let _ = child.kill();
        let _ = child.wait();
        panic!("the watchdog did not stop a daemon whose cycle was stuck");
    };
    assert_eq!(status.code(), Some(70), "{status:?}");
    assert!(
        started.elapsed() >= Duration::from_secs(5),
        "not before five intervals"
    );
    let sent = beats.lock().unwrap().len();
    assert_eq!(
        sent, 1,
        "one heartbeat for the one cycle that finished, none after"
    );
}

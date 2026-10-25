//! Network-path integration tests for the receiver-split anti backends
//! + notification bridge, against a real kevy-server over TCP.
//!
//! The `kevy_backends` / `kevy_notify` unit tests run over `mem://`
//! (kevy's in-process dispatch), which covers every command sequence
//! but not the actual `kevy://` socket path. These spin up the real
//! `ghcr.io/goliajp/kevy` server in a container and exercise rate +
//! auth (request/response) and notify (pub/sub) over TCP.
//!
//! greylist uses the identical `with_conn` request/response mechanism
//! as rate, and its trait lives in `mailrs_shield` (not reachable from
//! this integration crate), so it is covered by the `mem://` unit test.

use std::sync::Arc;
use std::time::Duration;

use testcontainers::ImageExt;
use testcontainers::{
    GenericImage,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

use mailrs_core::event_bus::{EventBus, SmtpEvent};
use mailrs_receiver::inbound::kevy_backends::{
    KevyServerGreylistBackend, KevyServerRateLimitStore,
};
use mailrs_receiver::inbound::rate_limit::{RateLimitStore, TokenBucketConfig};
use mailrs_receiver::kevy_net::KevyNetClient;
use mailrs_receiver::kevy_notify::{KevyEventPublisher, NOTIFY_CHANNEL, spawn_kevy_notify_bridge};

/// Which kevy-server these run against.
///
/// It was `latest`, and that hid the pair production would actually run: the
/// point of this default is that it names the tag in
/// `deploy/docker-compose.prod.yml`, so shipping a client the container has
/// never been tested against is a diff someone has to write rather than a
/// silent pull.
///
/// **So it moves with that compose file.** It did not, once: the compose went
/// to 6.x while this stayed at `3.18.0`, which tested client 6 against server
/// 3.18 — a pair production has never run — under a comment explaining why
/// that must not happen.
///
/// `MAILRS_TEST_KEVY_TAG` overrides it, so the same suite can be pointed at the
/// version prod runs and the version prod is going to.
fn kevy_tag() -> String {
    std::env::var("MAILRS_TEST_KEVY_TAG").unwrap_or_else(|_| "6.4.0".to_string())
}

/// Start a kevy-server container and return its `kevy://host:port` URL.
/// The container handle must stay alive for the duration of the test.
async fn setup_kevy() -> (testcontainers::ContainerAsync<GenericImage>, String) {
    // One container start at a time, workspace-wide: `cargo test
    // --workspace` runs every test binary in parallel and six fixtures
    // each start their own, which times out the wait-for-ready and
    // turns neighbours into failures. Startup only — running against a
    // live container in parallel is fine.
    let _startup = mailrs_test_docker::startup_lock().await;
    let tag = kevy_tag();
    // Configured the way production is, not the way the default is.
    //
    // The change feed is OFF by default on every kevy version — `feed_tail`
    // answers `ERR feed disabled (start with feed_enabled or replication on)` —
    // and there is no CLI flag or env var for it, only the TOML. Production
    // mounts `/apps/mailrs/kevy.toml` with `[feed] enabled = true` for exactly
    // this reason, and without that file a test would conclude the feed is
    // broken on a server where it works.
    //
    // Copied in rather than assumed, so what is under test is the pair
    // production runs: this client against that server, configured as deployed.
    let container = GenericImage::new("ghcr.io/goliajp/kevy", &tag)
        .with_wait_for(WaitFor::message_on_stderr("starting:"))
        .with_exposed_port(6379.tcp())
        .with_copy_to("/etc/kevy/kevy.toml", b"[feed]\nenabled = true\n".to_vec())
        .with_startup_timeout(mailrs_test_docker::STARTUP_TIMEOUT)
        .start()
        .await
        .expect("start kevy container");
    // kevy binds 0.0.0.0 (IPv4), so the client is pinned to it.
    let host = mailrs_test_docker::ipv4_loopback(
        &container
            .get_host()
            .await
            .expect("container host")
            .to_string(),
    );
    let port = container
        .get_host_port_ipv4(6379)
        .await
        .expect("container port");
    let url = format!("kevy://{host}:{port}");

    // poll until the server answers PING (boot can lag the log line).
    let mut tries = 0;
    loop {
        let c = Arc::new(KevyNetClient::new(url.clone()));
        let res = tokio::task::spawn_blocking(move || {
            c.with_conn(|conn| conn.ping().map_err(std::io::Error::other))
        })
        .await
        .expect("join");
        match res {
            Ok(()) => break,
            Err(e) => {
                tries += 1;
                if tries >= 50 {
                    panic!("kevy server never answered PING: {e}");
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    (container, url)
}

#[tokio::test]
async fn the_greylist_backend_over_tcp_meets_the_contract() {
    let (_container, url) = setup_kevy().await;
    let backend = KevyServerGreylistBackend::new(Arc::new(KevyNetClient::new(url)));
    mailrs_shield::greylist::check_greylist_backend(&backend).await;
}

#[tokio::test]
async fn rate_limit_over_tcp_allows_to_capacity_then_rejects() {
    let (_container, url) = setup_kevy().await;
    let client = Arc::new(KevyNetClient::new(url));
    let store = KevyServerRateLimitStore::new(
        client,
        TokenBucketConfig {
            capacity: 2,
            refill_rate: 0.0,
        },
    );

    assert!(store.check("10.0.0.1").await, "1st under capacity");
    assert!(store.check("10.0.0.1").await, "2nd at capacity");
    assert!(!store.check("10.0.0.1").await, "3rd over capacity");
    assert!(store.check("10.0.0.2").await, "distinct key unaffected");
}

/// The login guard's policy over a real kevy-server: the same scenarios
/// the in-process store passes.
#[tokio::test]
async fn login_guard_over_tcp_meets_the_contract() {
    let (_container, url) = setup_kevy().await;
    tokio::task::spawn_blocking(move || {
        mailrs_auth_guard::login::contract::check_ops(
            || {
                let mut c = kevy_client::Connection::connect(&url).expect("kevy");
                c.flushall().expect("flushall");
                c
            },
            |_, _| {},
        );
    })
    .await
    .expect("contract");
}

#[tokio::test]
async fn notify_bridge_delivers_over_tcp() {
    let (_container, url) = setup_kevy().await;
    let channel = NOTIFY_CHANNEL.to_vec();

    let sub_bus = EventBus::new(64);
    let mut rx = sub_bus.subscribe();
    spawn_kevy_notify_bridge(
        url.clone(),
        channel.clone(),
        "sub-origin".into(),
        sub_bus.clone(),
    );
    // let the bridge SUBSCRIBE before publishing.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let client = Arc::new(KevyNetClient::new(url));
    let publisher = KevyEventPublisher::new(client, channel, "pub-origin".into());
    publisher.publish(&SmtpEvent::NewMessage {
        user: "u".into(),
        thread_id: "t".into(),
        sender: "s".into(),
        subject: "tcp".into(),
        snippet: "sn".into(),
    });

    let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("bridge should deliver within timeout")
        .expect("recv ok");
    assert!(matches!(got.event, SmtpEvent::NewMessage { .. }));
}

/// The web UI's event stream works against the server this deploy will meet.
///
/// `webapi/src/handlers/events.rs` drives the WebSocket feed with
/// `conn.feed_read(...)` — the one command in the whole network-kevy surface
/// that is newer than the rest. Every other call site here uses commands kevy
/// has had for a long time.
///
/// That makes this the one place the untested pair could break. The workspace
/// is on kevy-client 2.2; production pins kevy-server **3.18.0**; shipping the
/// Rust side without moving the container puts those together, and neither
/// column of the benchmark covers it. If 3.18 does not know `FEED.READ`, the
/// event stream stops and the UI stops updating — quietly, because the loop
/// logs and retries.
///
/// Run against both:
///
/// ```text
/// MAILRS_TEST_KEVY_TAG=3.18.0 cargo test -p mailrs-server --test kevy_network
/// MAILRS_TEST_KEVY_TAG=5.1.0  cargo test -p mailrs-server --test kevy_network
/// ```
#[tokio::test]
async fn the_change_feed_answers_on_this_server() {
    let (_c, url) = setup_kevy().await;
    let mut conn = kevy_client::Connection::connect(&url).expect("connect");

    // Where the consumer starts, exactly as `events.rs` does it: the shard's
    // tail, then read forward from there.
    let (generation, offset) = conn
        .feed_tail(0)
        .unwrap_or_else(|e| panic!("FEED.TAIL rejected by this server: {e}"));

    // An empty feed is a fine answer; an unknown verb is not. What is being
    // asked is whether the server implements the command, not whether anything
    // has happened yet.
    let batch = conn
        .feed_read(0, generation, offset, Some(16), &[b"mailrs:" as &[u8]])
        .unwrap_or_else(|e| {
            panic!(
                "FEED.READ rejected by this server: {e}\n\
                 The web UI's event stream is built on this call. If the server \
                 under test is the one production runs, this deploy stops the \
                 stream."
            )
        });
    assert!(
        batch.frames.len() <= 16,
        "a bounded read must respect its bound; got {}",
        batch.frames.len()
    );
}

//! A kevy-server in a container, for code that needs what the in-process
//! `mem://` store does not have: WATCH / MULTI, pipelines, blocking pops.

use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

/// The tag production runs, as in `deploy/docker-compose.prod.yml`.
pub const TAG: &str = "6.4.0";

/// Which server stands in for the network kevy: `MAILRS_TEST_NETWORK_STORE`
/// set to `redis` (7.4) or `valkey` (9.0) runs the same tests against the
/// stores a deployment may use instead of kevy-server; unset, kevy-server.
fn image() -> (GenericImage, &'static str) {
    match std::env::var("MAILRS_TEST_NETWORK_STORE").as_deref() {
        Ok("redis") => (
            GenericImage::new("redis", "7.4-alpine")
                .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections")),
            "redis",
        ),
        Ok("valkey") => (
            GenericImage::new("valkey/valkey", "9.0-alpine")
                .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections")),
            "redis",
        ),
        Ok("kevy") | Err(_) => (
            GenericImage::new("ghcr.io/goliajp/kevy", TAG)
                .with_wait_for(WaitFor::message_on_stderr("starting:")),
            "kevy",
        ),
        Ok(other) => panic!("MAILRS_TEST_NETWORK_STORE={other}: kevy, redis or valkey"),
    }
}

/// A started network store and its URL, once it answers PING.
/// The container stops when the handle is dropped.
pub async fn kevy_server() -> (ContainerAsync<GenericImage>, String) {
    let startup = crate::startup_lock().await;
    let (image, scheme) = image();
    let container = image
        .with_exposed_port(6379.tcp())
        .with_startup_timeout(crate::STARTUP_TIMEOUT)
        .start()
        .await
        .expect("start kevy container");
    let host = crate::ipv4_loopback(&container.get_host().await.expect("host").to_string());
    let port = container.get_host_port_ipv4(6379).await.expect("port");
    drop(startup);
    let url = format!("{scheme}://{host}:{port}");
    for _ in 0..50 {
        let u = url.clone();
        let up = tokio::task::spawn_blocking(move || {
            kevy_client::Connection::connect(&u).and_then(|mut c| c.ping())
        })
        .await
        .expect("join");
        if up.is_ok() {
            return (container, url);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("kevy-server at {url} never answered");
}

/// Every message published on one channel from the moment [`listen`]
/// returns, in the order the server delivered them.
pub struct Published(std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>);

impl Published {
    pub fn all(&self) -> Vec<Vec<u8>> {
        self.0.lock().expect("published").clone()
    }
}

/// Subscribe to `channel` on the server at `url`; returns once the server
/// has confirmed the subscription, so nothing published after it is missed.
pub fn listen(url: &str, channel: &[u8]) -> Published {
    use kevy_client::PubsubEvent;
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (ready, subscribed) = std::sync::mpsc::channel();
    let (url, channel, sink) = (url.to_string(), channel.to_vec(), log.clone());
    std::thread::spawn(move || {
        let mut sub =
            kevy_client::Subscriber::connect_channels(&url, &[&channel]).expect("subscribe");
        while let Ok(event) = sub.recv() {
            match event {
                PubsubEvent::Subscribe { .. } => {
                    let _ = ready.send(());
                }
                PubsubEvent::Message { payload, .. } => {
                    sink.lock().expect("published").push(payload);
                }
                _ => {}
            }
        }
    });
    subscribed
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("subscription confirmed");
    Published(log)
}

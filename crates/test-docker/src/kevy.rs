//! A kevy-server in a container, for code that needs what the in-process
//! `mem://` store does not have: WATCH / MULTI, pipelines, blocking pops.

use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

/// The tag production runs, as in `deploy/docker-compose.prod.yml`.
pub const TAG: &str = "6.4.0";

/// A started kevy-server and its `kevy://` URL, once it answers PING.
/// The container stops when the handle is dropped.
pub async fn kevy_server() -> (ContainerAsync<GenericImage>, String) {
    let startup = crate::startup_lock().await;
    let container = GenericImage::new("ghcr.io/goliajp/kevy", TAG)
        .with_wait_for(WaitFor::message_on_stderr("starting:"))
        .with_exposed_port(6379.tcp())
        .with_startup_timeout(crate::STARTUP_TIMEOUT)
        .start()
        .await
        .expect("start kevy container");
    let host = crate::ipv4_loopback(&container.get_host().await.expect("host").to_string());
    let port = container.get_host_port_ipv4(6379).await.expect("port");
    drop(startup);
    let url = format!("kevy://{host}:{port}");
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

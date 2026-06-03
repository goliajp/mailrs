//! Ephemeral Kevy/Redis container for `RedisNotifier` integration tests.
//!
//! Same pattern as `pg.rs` — one container per call, handle held alive
//! by the caller so dropping it stops the container.

use testcontainers::ContainerAsync;
use testcontainers_modules::redis::Redis;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

/// Spin a fresh Redis container, return its handle plus the
/// `redis://host:port` URL ready to hand to `RedisNotifier::new(...)`.
pub async fn start_redis() -> (ContainerAsync<Redis>, String) {
    let container = Redis::default()
        .start()
        .await
        .expect("start redis container");
    let host = container
        .get_host()
        .await
        .expect("get container host")
        .to_string();
    let port = container
        .get_host_port_ipv4(6379)
        .await
        .expect("get container port");
    let url = format!("redis://{host}:{port}");
    (container, url)
}

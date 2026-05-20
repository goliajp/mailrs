//! Outbound mail queue primitives: DKIM signing, DSN generation, MTA-STS
//! lookup, retry/backoff, plus a pluggable store trait and a Postgres
//! reference implementation.
//!
//! `mailrs-outbound-queue` extracts the queue + delivery layer from
//! [mailrs] so it can be reused — or driven by a custom store — in any Rust
//! MTA. The "pure" pieces ([`dkim_sign`], [`dsn`], [`mta_sts`], [`retry`])
//! depend only on hashes / DNS / pure arithmetic; the [`QueueStore`] +
//! [`Notifier`] traits in [`store`] decouple delivery state from any
//! particular backend.
//!
//! # Feature flags
//!
//! | Feature | Default | Includes |
//! |---------|---------|----------|
//! | `pg`    | on      | [`PgQueueStore`] + [`RedisNotifier`] + the bundled [`DeliveryWorker`] that consumes them. Pulls in `sqlx` and `redis`. |
//!
//! Disable the `pg` feature if you want only the traits + pure primitives:
//!
//! ```toml
//! mailrs-outbound-queue = { version = "1", default-features = false }
//! ```
//!
//! # Two paths
//!
//! The crate exposes two parallel APIs against the same underlying queue
//! semantics:
//!
//! - **Trait API** ([`QueueStore`], [`Notifier`]) — the portable surface.
//!   Use this if you want a non-PG backend, or if you want your own
//!   delivery loop. An always-available [`InMemoryQueueStore`] is included
//!   for tests + pilots.
//! - **PG free functions** in the [`queue`] module — convenience for the
//!   common case where you already have a `sqlx::PgPool` and just want
//!   `queue::enqueue(pool, ...)`. These back the bundled
//!   [`DeliveryWorker`] and are what mailrs itself uses.
//!
//! Both APIs are stable for v1.x and back-compatible. The trait API plus a
//! generic worker is the target for v2.
//!
//! [mailrs]: https://github.com/goliajp/mailrs
//! [`mailrs-smtp-client`]: https://crates.io/crates/mailrs-smtp-client

use std::sync::Arc;

pub mod dkim_sign;
pub mod dsn;
pub mod mta_sts;
pub mod queue;
pub mod retry;
pub mod store;

#[cfg(feature = "pg")]
pub mod pg_store;
#[cfg(feature = "pg")]
pub mod worker;

pub use dkim_sign::DkimSignConfig;
pub use queue::{QueueStatus, QueuedMessage};
pub use retry::{retry_delay_secs, should_bounce};
pub use store::{
    InMemoryNotifier, InMemoryQueueStore, NoopNotifier, Notifier, QueueStore, StoreError,
};

#[cfg(feature = "pg")]
pub use pg_store::{PgQueueStore, RedisNotifier};
#[cfg(feature = "pg")]
pub use worker::{DeliveryWorker, WorkerConfig, group_by_domain};

/// Outbound delivery event for external observers (admin UI, audit log,
/// metrics pipeline).
#[derive(Debug, Clone)]
pub enum DeliveryEvent {
    /// A delivery attempt is starting for `queue_id` targeting `domain`.
    Attempt { queue_id: i64, domain: String },
    /// The message was accepted by the remote MX.
    Success { queue_id: i64, domain: String },
    /// A delivery attempt failed; the message is rescheduled for retry.
    Failed {
        queue_id: i64,
        domain: String,
        error: String,
    },
    /// The message bounced permanently and will not be retried; a DSN was
    /// queued back to the original `sender`.
    Bounced { queue_id: i64, sender: String },
}

/// Callback channel for [`DeliveryEvent`] notifications. Wrapped in `Arc` so
/// the worker can clone it across spawned delivery tasks.
pub type DeliveryEventSender = Arc<dyn Fn(DeliveryEvent) + Send + Sync>;

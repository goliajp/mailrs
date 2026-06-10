#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

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
//! | `pg`    | on      | [`PgQueueStore`] + [`KevyNotifier`] + the bundled [`DeliveryWorker`] that consumes them. Pulls in `sqlx` and `kevy-embedded`. |
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

/// DKIM signing helpers (RFC 6376): config + sign-and-prepend.
pub mod dkim_sign;
/// Delivery Status Notification (RFC 3464) formatting.
pub mod dsn;
/// MTA-STS (RFC 8461) policy lookup + caching.
pub mod mta_sts;
/// Queue row type, status enum, and retry-attempt bookkeeping.
pub mod queue;
/// Exponential-backoff retry math + "is it permanent?" classifier.
pub mod retry;
/// Pluggable [`QueueStore`] trait + an in-memory reference impl.
pub mod store;

/// Postgres-backed [`QueueStore`] implementation (feature-gated).
#[cfg(feature = "pg")]
pub mod pg_store;

/// Connection pool of the active SQL backend (PostgreSQL by default,
/// spg-embedded behind the `spg` feature).
#[cfg(all(feature = "pg", not(feature = "spg")))]
pub type BackendPool = sqlx::PgPool;
/// Connection pool of the active SQL backend (PostgreSQL by default,
/// spg-embedded behind the `spg` feature).
#[cfg(feature = "spg")]
pub type BackendPool = spg_sqlx::SpgPool;
/// Async delivery worker that drains the queue + dispatches via SMTP (feature-gated).
#[cfg(feature = "pg")]
pub mod worker;

pub use dkim_sign::{DkimDomainKey, DkimSignConfig};
pub use queue::{QueueStatus, QueuedMessage};
pub use retry::{retry_delay_secs, retry_delay_secs_jittered, should_bounce};
pub use store::{
    InMemoryNotifier, InMemoryQueueStore, NoopNotifier, Notifier, QueueStore, StoreError,
};

#[cfg(feature = "pg")]
pub use pg_store::{KevyNotifier, PgQueueStore};
#[cfg(feature = "pg")]
pub use worker::{DeliveryWorker, WorkerConfig, group_by_domain};

/// Outbound delivery event for external observers (admin UI, audit log,
/// metrics pipeline, TLSRPT reporter).
#[derive(Debug, Clone)]
pub enum DeliveryEvent {
    /// A delivery attempt is starting for `queue_id` targeting `domain`.
    Attempt {
        /// Queue row id being attempted.
        queue_id: i64,
        /// Destination domain for this attempt.
        domain: String,
    },
    /// The STARTTLS phase completed (success or failure). Emitted
    /// once per MX connection right after the TLS handshake, before
    /// any RCPT TO / DATA. Carries structured outcome suitable for
    /// TLSRPT (RFC 8460) reporting.
    ///
    /// Not emitted when the connection is plain (no STARTTLS
    /// attempted), in which case the caller should record the
    /// session as untrusted-TLS via its own logging.
    TlsAttempt {
        /// Destination domain (the recipient's, not the MX's).
        domain: String,
        /// MX hostname we connected to.
        mx_host: String,
        /// Structured outcome of the TLS attempt.
        outcome: TlsAttemptOutcome,
    },
    /// The message was accepted by the remote MX.
    Success {
        /// Queue row id that just succeeded.
        queue_id: i64,
        /// Destination domain that accepted the message.
        domain: String,
    },
    /// A delivery attempt failed; the message is rescheduled for retry.
    Failed {
        /// Queue row id that failed this attempt.
        queue_id: i64,
        /// Destination domain that was attempted.
        domain: String,
        /// Human-readable error from the SMTP client (typically the
        /// remote's response text).
        error: String,
    },
    /// The message bounced permanently and will not be retried; a DSN was
    /// queued back to the original `sender`.
    Bounced {
        /// Queue row id that bounced.
        queue_id: i64,
        /// Original envelope sender — the DSN gets queued back to them.
        sender: String,
    },
}

/// Outcome of one STARTTLS attempt, carried by
/// [`DeliveryEvent::TlsAttempt`]. The four variants discriminate
/// between TLS success, server-side refusal (still safely usable
/// in plain), and handshake failure (with the structured
/// [`mailrs_smtp_client::TlsOutcome`] underneath).
#[derive(Debug, Clone)]
pub enum TlsAttemptOutcome {
    /// STARTTLS completed; encrypted channel established.
    /// `policy` tells the report which policy gated this attempt
    /// (`"dane"`, `"sts"`, or `"opportunistic"`).
    Success {
        /// Which policy class was active for this attempt.
        policy: &'static str,
    },
    /// Server did not advertise STARTTLS in the EHLO response.
    /// Maps to RFC 8460 `starttls-not-supported`.
    NotAdvertised,
    /// Server rejected the `STARTTLS` command (returned 4xx/5xx).
    Rejected {
        /// SMTP response code.
        code: u16,
        /// SMTP response text.
        message: String,
    },
    /// TLS handshake started but failed mid-way. The wrapped
    /// [`mailrs_smtp_client::TlsOutcome`] is RFC 8460 §4.3-aligned.
    HandshakeFailed(mailrs_smtp_client::TlsOutcome),
}

/// Callback channel for [`DeliveryEvent`] notifications. Wrapped in `Arc` so
/// the worker can clone it across spawned delivery tasks.
pub type DeliveryEventSender = Arc<dyn Fn(DeliveryEvent) + Send + Sync>;

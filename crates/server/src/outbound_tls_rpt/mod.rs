//! Server-side observer that feeds outbound delivery events into
//! a persistent TLSRPT [`Store`]. RFC 8460 SMTP TLS Reporting.
//!
//! ## What this records
//!
//! Each `mailrs_outbound_queue::DeliveryEvent::TlsAttempt` becomes
//! one [`EventFact`] appended to the store with the current
//! recorded-at timestamp. Classification comes from the structured
//! [`mailrs_outbound_queue::TlsAttemptOutcome`] — no error-string
//! keyword matching anywhere in this path.
//!
//! ## Persistence
//!
//! Since 1.x, the observer is just a thin facade over a
//! [`mailrs_tls_rpt::Store`]. The `PgTlsRptStore` impl writes
//! every event into `tls_rpt_events` (an append-only PG table —
//! see `scripts/migrate-036-tls-rpt-events.sql`). Daily flush
//! drains the window, rebuilds the report, and submits.
//!
//! Restart-safe: events recorded before a crash survive in PG and
//! the next flush picks them up. The in-process observer carries
//! no state of its own.
//!
//! ## Submission
//!
//! [`submit_report`] performs the per-policy-domain submission:
//! lookup `_smtp._tls.<domain>` TXT, parse into a
//! [`mailrs_tls_rpt::TlsRptRecord`], then for each `rua` endpoint
//! either enqueue an outbound email (mailto:) or HTTPS POST the
//! gzipped report (https:). Per-endpoint failures are logged but
//! don't abort other endpoints' submission.

mod convert;
mod observer;
mod store;
mod submit;

pub use observer::TlsRptObserver;
pub use store::PgTlsRptStore;
pub use submit::submit_report;

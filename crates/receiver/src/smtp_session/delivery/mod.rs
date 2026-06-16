//! Delivery helpers — recipient resolution, sieve evaluation, and outbound
//! relay — parameterized by [`super::DeliveryDeps`] rather than the full
//! `ConnectionContext` (P6-S7). Both the monolith DATA handler (inline) and
//! the core spool consumer (after fetching a spool file) run these against
//! their own deps, so there is one delivery code path with two callers.

mod recipients;
mod remote;
mod sieve;

pub use recipients::classify_recipients;
pub use remote::{RemoteEnqueueResult, enqueue_remote_rcpts};
pub use sieve::apply_sieve_actions;

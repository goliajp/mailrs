//! Wire contract crate for the mailrs 4-process split.
//!
//! See `docs/CURRENT_STATE_FROZEN.md` §0.4 + §0.5 for the full method
//! surface this crate must cover.
//!
//! ## Architecture
//!
//! Four binaries communicate via this crate's RPC types:
//!
//! ```text
//!   receiver ──spool+kevy──> core ──HTTP──> webapi
//!                              │             │
//!                              │             ▼
//!                              └──HTTP────  sender
//! ```
//!
//! `core` and `fastcore` are alternate implementations of the same
//! `mailrs-core-api::server` trait — webapi/sender are agnostic to
//! which is running.
//!
//! ## Module map
//!
//! - `error` — `CoreApiError` + HTTP status mapping
//! - `types` — re-exports from `mailrs-mailbox` + wire-only types
//! - `method` — RPC method definitions: path + req/resp types
//! - `client` — `reqwest`-based async client (feature `client`)
//! - `server` — `axum`-based handler scaffolding (feature `server`)

// missing-docs is intentionally NOT denied at the crate level during
// Phase 1 scaffolding — sub-modules are stubs that will gain full doc
// coverage as request/response types fill in (checklist 1.5/1.6).
#![allow(missing_docs)]

pub mod error;
pub mod method;
pub mod types;

#[cfg(feature = "client")]
pub mod client;
// Split out of `client.rs` on 2026-08-02 by subject. Each carries a
// second `impl CoreClient` block, so the methods stay on the one type.
#[cfg(feature = "client")]
mod client_accounts;
#[cfg(feature = "client")]
mod client_messages;
#[cfg(feature = "client")]
mod client_outbound;
#[cfg(feature = "client")]
mod client_sidestate;
#[cfg(feature = "client")]
mod client_threads;

#[cfg(feature = "server")]
pub mod server;

/// API version. Bump on breaking change. Path prefix is `/v1/...`.
pub const API_VERSION: &str = "v1";

/// Default port mailrs-core listens on for internal RPC.
/// Override with `MAILRS_CORE_RPC_ADDR` env var.
pub const DEFAULT_CORE_RPC_PORT: u16 = 3300;

/// Env var name carrying the shared internal auth secret.
/// All 4 processes (core/fastcore/webapi/sender/receiver-if-needed) must
/// share the same value. Verified per request via `Authorization: Bearer`.
pub const AUTH_SECRET_ENV: &str = "MAILRS_CORE_API_SECRET";

//! Per-RPC handlers that thread `mailrs-core-api` requests into existing
//! `PgMailboxStore` / `DomainStore` inherent methods.
//!
//! Each submodule mirrors a `mailrs_core_api::method::*` module. Handlers
//! are kept thin — they convert axum Path/Query/Json extractors into the
//! 67/57 inherent method args, then convert results back through the
//! `From` wire conversions in `mailrs-core-api::types::*Wire`.
//!
//! All handlers return `Result<Json<T>, axum::http::StatusCode>` so the
//! axum router can map errors to wire status codes (404 / 409 / 500 / etc.).
//! Phase 2 stub — full error mapping (CoreApiError → status) in 2.5.

pub mod admin;
pub mod analysis;
pub mod contact;
pub mod conversation;
pub mod drafts;
pub mod mailbox;
pub mod message;
pub mod outbound;
pub mod reactions;
pub mod signatures;
pub mod templates;
pub mod thread;
pub mod webhooks;

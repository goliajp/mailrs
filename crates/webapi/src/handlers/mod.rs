//! REST + MCP handlers. Phase 3 — each handler is a thin shim that
//! delegates to `state.core_client.X()` RPC calls.

pub mod admin;
pub mod auth;
pub mod complete;
pub mod conversations;
pub mod events;
pub mod inline;
pub mod kevy_util;
pub mod mail;
pub mod messages;
pub mod misc;
pub mod prefs;

//! REST + MCP handlers. Phase 3 — each handler is a thin shim that
//! delegates to `state.core_client.X()` RPC calls.

pub mod auth;
pub mod conversations;
pub mod mail;

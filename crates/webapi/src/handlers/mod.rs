//! REST + MCP handlers. Phase 3 — each handler is a thin shim that
//! delegates to `state.core_client.X()` RPC calls.

pub mod admin;
pub mod audit;
pub mod auth;
pub mod autodiscover;
pub mod calendar;
pub mod complete;
pub mod conversations;
pub mod dav;
pub mod events;
pub mod inline;
pub mod invites;
pub mod jmap;
pub mod kevy_util;
pub mod keys;
pub mod mail;
pub mod mcp;
pub mod messages;
pub mod metrics;
pub mod misc;
pub mod oidc;
pub mod prefs;
pub mod search;
pub mod totp_util;

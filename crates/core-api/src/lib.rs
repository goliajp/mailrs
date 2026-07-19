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

/// Per-user Meilisearch index name.
///
/// Lives here because the writer (`mailrs-fastcore`'s `live_sync`) and
/// the reader (`mailrs-webapi`'s search handler) are separate binaries
/// that must agree on it. They previously did not: the writer indexed
/// into `mailrs_<user>` while the search handler queried
/// `conversations-<user>`, an index that never existed. Meili answered
/// 404, the handler swallowed the error and fell through to its linear
/// fallback, and conversation search returned an empty list for every
/// query on prod for weeks (reported 2026-07-19).
///
/// `@` becomes `_at_` and every other non-`[A-Za-z0-9_-]` byte becomes
/// `_`, satisfying Meili's `^[a-zA-Z0-9_-]{1,400}$` index-uid rule.
pub fn meili_index_name(user: &str) -> String {
    let mut out = String::from("mailrs_");
    for c in user.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else if c == '@' {
            out.push_str("_at_");
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod meili_index_tests {
    use super::meili_index_name;

    #[test]
    fn matches_the_names_live_on_prod() {
        // Captured from the running Meili instance 2026-07-19 — these
        // are the indexes the writer has actually created.
        assert_eq!(
            meili_index_name("lihao@golia.jp"),
            "mailrs_lihao_at_golia_jp"
        );
        assert_eq!(
            meili_index_name("admin@golia.jp"),
            "mailrs_admin_at_golia_jp"
        );
        assert_eq!(meili_index_name("ggi@golia.jp"), "mailrs_ggi_at_golia_jp");
    }

    #[test]
    fn output_is_a_legal_meili_uid() {
        let uid = meili_index_name("a.b+c@x-y.example");
        assert!(
            uid.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "got {uid}"
        );
    }
}

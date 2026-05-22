//! ACME orchestration re-exported from
//! [`mailrs-acme`](https://crates.io/crates/mailrs-acme).
//!
//! Carved out as a generic stone — the orchestration layer is useful
//! for any Rust web server doing Let's Encrypt automation, not just
//! mail. The full mailrs-acme surface remains accessible via the
//! `mailrs_acme::*` path; this shim only re-exports what main.rs uses.

#[allow(unused_imports)]
pub use mailrs_acme::{
    init, spawn_challenge_server, spawn_renewal_task, ChallengeTokens, RenewalConfig,
};

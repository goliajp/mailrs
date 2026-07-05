//! Core-api contract handlers that make fastcore serve the SAME route
//! surface as the pg-core, so the two backends are functionally identical
//! (v2 point 3). Split by family, one file each, to keep every file under
//! the 500-line limit — `lib.rs` is already oversized.
//!
//! Backing regimes:
//! - `prefs` (drafts / signatures / templates): shared side-state in the
//!   INDEPENDENT network kevy (the same keys webapi + pg-core read), so
//!   both cores serve them identically. Reached via `state.net_conn()`.

pub mod admin_state;
pub mod analysis;
pub mod contacts;
pub mod mail_admin;
pub mod prefs;

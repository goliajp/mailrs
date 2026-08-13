//! Tests for the core-RPC surface, moved out of `mod.rs` on 2026-08-02
//! where they were 663 of its 1,254 lines.
//!
//! Named modules rather than one trailing `mod tests`, so `file-size.md`
//! counts them — which is the right answer here: they are different kinds
//! of test (in-process pg-core coverage, and a sync test that needs a real
//! database) and they were only together because they shared a file.
//!
//! The route-parity lock that used to live here as `parity.rs` is now
//! `scripts/check-core-parity.sh`. It moved out of Rust because every way
//! it had rotted came from being in Rust behind a feature: it named both
//! router files by path (both had moved, and one path did not exist at
//! all), and it only compiled under `--features core-rpc`, which nothing
//! built after 2026-08-02. The script finds the routers by content and
//! runs in the everyday gate.

#![cfg(test)]

mod peer_roles;
mod pg_core;
mod real_pg_sync;
mod two_lane;

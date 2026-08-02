//! Tests for the core-RPC surface, moved out of `mod.rs` on 2026-08-02
//! where they were 663 of its 1,254 lines.
//!
//! Named modules rather than one trailing `mod tests`, so `file-size.md`
//! counts them — which is the right answer here: they are three different
//! kinds of test (a route-parity lock, in-process pg-core coverage, and a
//! sync test that needs a real database) and they were only together
//! because they shared a file.

#![cfg(test)]

mod parity;
mod pg_core;
mod real_pg_sync;

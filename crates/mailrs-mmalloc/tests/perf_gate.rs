//! Per-operation performance budget gate.
//!
//! Placeholder in M0 — populated with concrete budgets in M7 once the bench
//! suite has measured the finished allocator's p95 on the dev/CI machine.
//! Each test below asserts `elapsed < budget` for one hot operation; CI's
//! `cargo test --workspace` then catches any regression that pushes a path
//! over its 3×-headroom budget.
//!
//! Why integration tests, not criterion: criterion warns on regression but
//! doesn't fail CI. Integration tests with `assert!(elapsed < budget)` do.
//! See `goliajp/tora`'s perf_gate.rs pattern; same shape used here.
//!
//! Linux-only — perf on macOS host_stub is `std::alloc::System` so the
//! numbers wouldn't reflect anything about mmalloc.

#![cfg(target_os = "linux")]

// Real assertions land in M7. Empty marker test keeps this file compiled
// and discoverable so M7 just adds bodies, doesn't reshape the file.
#[test]
fn perf_gate_placeholder_compiles() {}

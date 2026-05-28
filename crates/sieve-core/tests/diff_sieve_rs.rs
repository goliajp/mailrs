//! ckpt 4 sieve-core — differential test against Stalwart's
//! `sieve-rs` (the oracle this engine will eventually replace).
//!
//! Builds the same action sequence out of both engines (mapped to
//! a shared `NormalizedAction` enum that drops engine-specific
//! metadata) and asserts equality. The corpus + framework live in
//! `tests/common/`; this file is just the entry point.
//!
//! ckpt 4 → 5 trigger gate: 200/200 corpus rows agree. Slice 4
//! lands at 100/200 (50%).

mod common;

use common::{corpus::corpus, ours, sieve_rs};

#[test]
fn engines_agree_on_corpus() {
    let mut disagreements = Vec::new();
    for (label, script, msg) in corpus() {
        let a = ours(script, msg);
        let b = sieve_rs(script, msg);
        if a != b {
            disagreements.push((label, a, b));
        }
    }
    assert!(
        disagreements.is_empty(),
        "engine disagreement: {disagreements:#?}",
    );
}

//! sieve-core golden regression test.
//!
//! Freezes the engine's output across the corpus as
//! `tests/common/golden.txt`. The corpus was validated == Stalwart
//! `sieve-rs` across 245 rows during the v8 ckpt 4-5 parity ramp; the
//! AGPL oracle was dropped in ckpt 6, so this is now a pure golden
//! regression. Regenerate via the `gen_golden` test (`--ignored`)
//! after an intentional behavior change.

mod common;

use common::{
    corpus::{corpus, envelope_corpus},
    ours, ours_with_envelope,
};

const GOLDEN: &str = include_str!("common/golden.txt");

#[test]
fn ours_matches_golden() {
    let golden: Vec<&str> = GOLDEN.lines().collect();
    let mut i = 0;
    for (label, script, msg) in corpus() {
        let got = format!("{label}\t{:?}", ours(script, msg));
        assert_eq!(
            got, golden[i],
            "corpus row {i} ({label}) drifted from golden"
        );
        i += 1;
    }
    for (label, script, msg, env) in envelope_corpus() {
        let got = format!("{label}\t{:?}", ours_with_envelope(script, msg, env));
        assert_eq!(
            got, golden[i],
            "envelope row {i} ({label}) drifted from golden"
        );
        i += 1;
    }
    assert_eq!(i, golden.len(), "corpus size != golden line count");
}

/// One-off generator (run with `--ignored`) that freezes the current
/// sieve-core output as `tests/common/golden.txt`. Re-run only when an
/// intentional behavior change updates expected output.
#[test]
#[ignore = "regenerates golden.txt"]
fn gen_golden() {
    use std::fmt::Write;
    let mut out = String::new();
    for (label, script, msg) in corpus() {
        writeln!(out, "{label}\t{:?}", ours(script, msg)).unwrap();
    }
    for (label, script, msg, env) in envelope_corpus() {
        writeln!(out, "{label}\t{:?}", ours_with_envelope(script, msg, env)).unwrap();
    }
    std::fs::write("tests/common/golden.txt", out).unwrap();
}

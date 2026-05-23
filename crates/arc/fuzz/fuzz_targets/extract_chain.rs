#![no_main]
//! Fuzz the chain extractor — full RFC 5322 header unfold + per-instance
//! grouping. Highest-attack-surface entry point in the crate.

use libfuzzer_sys::fuzz_target;
use mailrs_arc::ArcChain;

fuzz_target!(|data: &[u8]| {
    let _ = ArcChain::extract(data);
});

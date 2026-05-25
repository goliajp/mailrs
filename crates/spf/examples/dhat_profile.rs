//! dhat-heap mem profile for `mailrs_spf::Record::parse`.
//!
//! Three inputs (simple, complex_8, pathological_8) — same shapes the
//! `compare_mail_auth` bench uses. Run produces `dhat-heap.json`.
//!
//! Run: `cargo run --example dhat_profile -p mailrs-spf --release`

use mailrs_spf::Record;
use std::hint::black_box;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const SIMPLE: &str = "v=spf1 ip4:203.0.113.0/24 -all";
const COMPLEX_8: &str = "v=spf1 ip4:1.2.3.0/24 ip4:5.6.7.0/24 \
    ip4:10.0.0.0/8 ip4:172.16.0.0/12 a:mail.example.com \
    mx:example.com include:_spf.google.com include:spf.example.com -all";
const PATHOLOGICAL_8: &str = "v=spf1 include:a.example.com \
    include:b.example.com include:c.example.com include:d.example.com \
    include:e.example.com include:f.example.com include:g.example.com \
    include:h.example.com -all";

fn main() {
    let _profiler = dhat::Profiler::new_heap();
    for _ in 0..10_000 {
        black_box(Record::parse(black_box(SIMPLE)).unwrap());
        black_box(Record::parse(black_box(COMPLEX_8)).unwrap());
        black_box(Record::parse(black_box(PATHOLOGICAL_8)).unwrap());
    }
}

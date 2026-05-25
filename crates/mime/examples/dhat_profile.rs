//! dhat-heap mem profile for `mailrs_mime::parse`.
//!
//! Exercises the same INVITE input used by `vs_mail_parser/find_calendar`
//! N times and writes `dhat-heap.json` next to the binary. Open it with
//! https://github.com/nnethercote/dhat to see total alloc bytes, peak,
//! and call count.
//!
//! Run: `cargo run --example dhat_profile -p mailrs-mime --release`

use mailrs_mime::parse;
use std::hint::black_box;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const INVITE: &[u8] = b"Content-Type: multipart/alternative; boundary=\"x\"\r\n\
\r\n\
--x\r\n\
Content-Type: text/plain\r\n\
\r\n\
Meeting invitation\r\n\
--x\r\n\
Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\
\r\n\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n\
--x--\r\n";

fn main() {
    let _profiler = dhat::Profiler::new_heap();
    // 10_000 iterations is enough to amortise startup noise; per-call
    // numbers fall out of the json totals.
    for _ in 0..10_000 {
        let p = parse(black_box(INVITE));
        let cal = p.find_by_content_type("text/calendar");
        black_box(cal.map(|x| x.body.len()));
    }
}

//! mailrs-ical iCalendar parser — bench-harness runner.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: ical <corpus.ics> <iterations>");
        std::process::exit(1);
    }
    let text = std::fs::read_to_string(&args[1]).expect("read corpus");
    let iters: u64 = args[2].parse().expect("iterations");

    let t0 = Instant::now();
    for _ in 0..iters {
        let _ = std::hint::black_box(mailrs_ical::parse::parse_calendar(std::hint::black_box(
            &text,
        )));
    }
    let elapsed = t0.elapsed();
    let ns_per_op = elapsed.as_nanos() as f64 / iters as f64;
    println!("rust/mailrs-ical/parse: {:.1} ns/op ({} iters)", ns_per_op, iters);
}

//! mailrs-rfc5322 — bench-harness runner. Equivalent op: read-message + Subject + From extraction.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: rfc5322 <corpus.eml> <iterations>");
        std::process::exit(1);
    }
    let bytes = std::fs::read(&args[1]).expect("read corpus");
    let iters: u64 = args[2].parse().expect("iterations");

    let t0 = Instant::now();
    for _ in 0..iters {
        let msg = std::hint::black_box(mailrs_rfc5322::Message::new(&bytes));
        let _ = std::hint::black_box(msg.header("Subject"));
        let _ = std::hint::black_box(msg.header("From"));
    }
    let elapsed = t0.elapsed();
    let ns_per_op = elapsed.as_nanos() as f64 / iters as f64;
    println!(
        "rust/mailrs-rfc5322/parse+subject+from: {:.1} ns/op ({} iters)",
        ns_per_op, iters
    );
}

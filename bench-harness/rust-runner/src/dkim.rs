//! mailrs-dkim DKIM-Signature parser — bench-harness runner.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: dkim <corpus.txt> <iterations>");
        std::process::exit(1);
    }
    let record = std::fs::read_to_string(&args[1]).expect("read corpus");
    let record = record.trim_end_matches('\n');
    let iters: u64 = args[2].parse().expect("iterations");

    let t0 = Instant::now();
    for _ in 0..iters {
        let _ = std::hint::black_box(mailrs_dkim::header::DkimHeader::parse(
            std::hint::black_box(record),
        ));
    }
    let elapsed = t0.elapsed();
    let ns_per_op = elapsed.as_nanos() as f64 / iters as f64;
    println!("rust/mailrs-dkim/parse: {:.1} ns/op ({} iters)", ns_per_op, iters);
}

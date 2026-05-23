//! mailrs-mime — bench-harness runner. Parses MIME tree.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: mime <corpus.eml> <iterations>");
        std::process::exit(1);
    }
    let bytes = std::fs::read(&args[1]).expect("read corpus");
    let iters: u64 = args[2].parse().expect("iterations");

    let t0 = Instant::now();
    for _ in 0..iters {
        let _ = std::hint::black_box(mailrs_mime::parse(std::hint::black_box(&bytes)));
    }
    let elapsed = t0.elapsed();
    let ns_per_op = elapsed.as_nanos() as f64 / iters as f64;
    println!("rust/mailrs-mime/parse: {:.1} ns/op ({} iters)", ns_per_op, iters);
}

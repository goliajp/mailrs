//! Throughput comparison: `Maildir::deliver` (N × per-message fsync)
//! vs `Maildir::deliver_batch` (single dir fsync for the whole batch).
//!
//! Designed to answer: does batched fsync actually help on the host
//! filesystem? On APFS / ext4 / btrfs we expect 3-10×. Run on the
//! target deployment FS to confirm before committing API to the
//! production delivery loop.
//!
//! Run: `cargo bench -p mailrs-maildir --bench deliver`

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_maildir::Maildir;

const MSG: &[u8] = b"From: alice@example.com\r\n\
                     To: bob@example.com\r\n\
                     Subject: Test message for fsync bench\r\n\
                     Date: Sat, 24 May 2026 12:00:00 +0000\r\n\
                     Message-ID: <bench@example.com>\r\n\
                     \r\n\
                     This is a typical-sized mail body. About 200 bytes.\r\n\
                     Realistic for transactional mail (notifications,\r\n\
                     receipts, password resets, etc).\r\n";

/// Per-message deliver: N fsync syscalls (one per message).
fn bench_deliver_loop(c: &mut Criterion) {
    for &n in &[1usize, 8, 64] {
        c.bench_function(&format!("deliver_loop/n={n}"), |b| {
            b.iter_custom(|iters| {
                let mut total = std::time::Duration::ZERO;
                for _ in 0..iters {
                    let dir = tempfile::tempdir().unwrap();
                    let md = Maildir::create(dir.path()).unwrap();
                    let start = std::time::Instant::now();
                    for _ in 0..n {
                        black_box(md.deliver(black_box(MSG)).unwrap());
                    }
                    total += start.elapsed();
                }
                total
            });
        });
    }
}

/// Batched deliver: 2 fsync syscalls total (tmp dir + new dir),
/// regardless of N.
fn bench_deliver_batch(c: &mut Criterion) {
    for &n in &[1usize, 8, 64] {
        let msgs: Vec<&[u8]> = (0..n).map(|_| MSG).collect();
        c.bench_function(&format!("deliver_batch/n={n}"), |b| {
            b.iter_custom(|iters| {
                let mut total = std::time::Duration::ZERO;
                for _ in 0..iters {
                    let dir = tempfile::tempdir().unwrap();
                    let md = Maildir::create(dir.path()).unwrap();
                    let start = std::time::Instant::now();
                    black_box(md.deliver_batch(black_box(&msgs)).unwrap());
                    total += start.elapsed();
                }
                total
            });
        });
    }
}

criterion_group!(benches, bench_deliver_loop, bench_deliver_batch);
criterion_main!(benches);

//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).
//!
//! DKIM signing is intentionally NOT gated here — RSA signing in debug
//! mode is 50-100× slower than release, which would make a wall-clock
//! budget either useless or require running tests in `--release`. The
//! criterion bench in `benches/core.rs` covers that path; this file
//! enforces the cheap algorithmic paths only.

use std::time::{Duration, Instant};

use mailrs_outbound_queue::dsn::format_dsn;
use mailrs_outbound_queue::mta_sts::mx_matches_policy;
use mailrs_outbound_queue::queue::is_hard_bounce;
use mailrs_outbound_queue::retry::{retry_delay_secs, should_bounce};

const ITERS: usize = 100;

fn time_median<F: FnMut()>(mut op: F) -> Duration {
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op();
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

#[test]
fn retry_delay_full_sequence_under_budget() {
    let median = time_median(|| {
        for attempt in 0..10 {
            let _ = retry_delay_secs(attempt);
        }
    });
    // Budget: 10 µs. Observed P95: ~100 ns (pure arithmetic).
    let budget = Duration::from_micros(10);
    assert!(
        median < budget,
        "retry_delay_secs(0..10) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn should_bounce_full_sequence_under_budget() {
    let median = time_median(|| {
        for attempt in 0..10 {
            let _ = should_bounce(attempt, 5);
        }
    });
    // Budget: 10 µs. Observed P95: ~50 ns.
    let budget = Duration::from_micros(10);
    assert!(
        median < budget,
        "should_bounce(0..10, 5) median {median:?} exceeded {budget:?}"
    );
}

// ===== MTA-STS MX-pattern matcher =====
//
// `mx_matches_policy` runs once per outbound delivery (post cache-miss
// or per-MX during enforcement). Two shapes matter: an exact match
// against a single pattern (cheap path) and a wildcard match against a
// multi-pattern policy (the realistic worst case for big providers like
// Google / Microsoft). Both are pure string ops — `to_lowercase()` + a
// linear scan with `ends_with`/`==` per pattern.

#[test]
fn mx_matches_policy_exact_x100_under_budget() {
    // Single call sits below the timer floor (<100 ns), so batch 100 per
    // sample to get a stable measurement. Realistic load: a delivery
    // worker processing a queue burst can run this hundreds of times in
    // quick succession across distinct destinations.
    let policy: [&str; 1] = ["mail.example.com"];
    let median = time_median(|| {
        for _ in 0..100 {
            let _ = mx_matches_policy("mail.example.com", &policy);
        }
    });
    // Budget: 300 µs (~20× headroom). Observed P95 (dev): ~14 µs for 100
    // calls (≈ 140 ns per call). Cost per call is dominated by
    // `to_lowercase()` allocating two small `String`s. An O(n²) variant
    // or a per-call regex compile would trip this gate immediately.
    let budget = Duration::from_micros(300);
    assert!(
        median < budget,
        "mx_matches_policy (exact x100) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn mx_matches_policy_wildcard_against_five_patterns_under_budget() {
    // Realistic shape: Google's MTA-STS policy has both an exact entry and
    // a wildcard; large providers list 3-5 patterns total.
    let policy: [&str; 5] = [
        "primary.mx.example.com",
        "secondary.mx.example.com",
        "tertiary.mx.example.com",
        "*.mx.example.com",
        "*.mail.protection.example.net",
    ];
    let median = time_median(|| {
        // Match against the wildcard (the 4th pattern) — forces the matcher
        // to walk past the three exact patterns first.
        let _ = mx_matches_policy("mx1.mx.example.com", &policy);
    });
    // Budget: 30 µs (~20× headroom). Observed P95 (dev): ~1.5 µs. Walks
    // 5 patterns, each requiring a `to_lowercase()` + `ends_with` /
    // equality check. The wildcard match also slices the host string.
    let budget = Duration::from_micros(30);
    assert!(
        median < budget,
        "mx_matches_policy (wildcard x5) median {median:?} exceeded {budget:?}"
    );
}

// ===== is_hard_bounce — per-failed-delivery classifier =====
//
// Called by the delivery worker every time the remote MX rejects a
// message, to decide whether the row flips to Bounced (5xx — permanent)
// or stays Failed for retry (4xx — temporary). Pure string trim +
// prefix check.
#[test]
fn is_hard_bounce_x100_under_budget() {
    // Batched (100×) to clear the timer floor; per-call cost is ~3 ns
    // (trim + prefix check on a short string).
    let median = time_median(|| {
        for _ in 0..100 {
            // realistic shape — a 5xx response with extended status code
            let _ = is_hard_bounce("550 5.1.1 <bob@example.com> user unknown");
        }
    });
    // Budget: 200 µs (~22× headroom). Observed P95 (dev): ~9 µs for 100
    // calls (≈ 90 ns per call). Pure `trim()` + `starts_with()`. If this
    // ever exceeds the budget it means someone added a regex/parse step
    // on what should be a 3-character look.
    let budget = Duration::from_micros(200);
    assert!(
        median < budget,
        "is_hard_bounce x100 median {median:?} exceeded {budget:?}"
    );
}

// ===== format_dsn — per-bounce DSN body builder =====
//
// `format_dsn` runs once per bounced delivery to produce the RFC 3464
// multipart DSN body that gets re-enqueued to the original sender. It
// does include one `chrono::Utc::now()` call (for the Date/Message-ID
// headers) — that's a syscall + format on every invocation, so the
// budget includes its cost.
#[test]
fn format_dsn_under_budget() {
    let median = time_median(|| {
        let _ = format_dsn(
            "mx.example.com",
            "alice@example.com",
            "bob@remote.example.org",
            "550 5.1.1 <bob@remote.example.org> user unknown",
            Some("original-msg-abc@example.com"),
        );
    });
    // Budget: 400 µs (~30× headroom). Observed P95 (dev): ~13 µs. The cost
    // is split between ~15 small `write!` calls into a `String` (each
    // potentially reallocating) and the chrono format step. Any
    // regression that adds a sync I/O call or a heavy template engine
    // will trip this gate.
    let budget = Duration::from_micros(400);
    assert!(
        median < budget,
        "format_dsn median {median:?} exceeded {budget:?}"
    );
}

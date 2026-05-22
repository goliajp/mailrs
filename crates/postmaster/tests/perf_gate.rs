//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_postmaster::{
    extract_bimi_logo_url, extract_tlsrpt_rua, parse_mta_sts_policy, validate_tlsrpt_record,
};

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
fn extract_bimi_logo_url_under_budget() {
    let record = "v=BIMI1; l=https://example.com/logo.svg; a=https://example.com/cert.pem";
    let median = time_median(|| {
        let _ = extract_bimi_logo_url(record);
    });
    // Budget: 20 µs. Observed P95: ~200 ns.
    let budget = Duration::from_micros(20);
    assert!(median < budget, "extract_bimi_logo_url median {median:?} exceeded {budget:?}");
}

// ===== MTA-STS policy body parser =====
//
// `parse_mta_sts_policy` is the pure body-of-the-text parser called once
// per MTA-STS policy fetch (per domain, post HTTP roundtrip). The
// realistic input has 4-6 lines: `version: STSv1`, `mode: enforce`, 1-3
// `mx: pattern` lines, `max_age: N`. The function does `lines() → trim →
// lower-case key → split-on-colon`, allocating two small `String`s per
// line.
#[test]
fn parse_mta_sts_policy_typical_under_budget() {
    let body = "version: STSv1\r\n\
                mode: enforce\r\n\
                mx: mail.example.com\r\n\
                mx: backup.example.com\r\n\
                mx: *.fallback.example.net\r\n\
                max_age: 86400\r\n";
    let median = time_median(|| {
        let _ = parse_mta_sts_policy(body);
    });
    // Budget: 100 µs (~30× headroom). Observed P95 (dev): ~3.5 µs. The
    // cost is dominated by the 6 `to_lowercase()` calls (one per key)
    // + the 12 `String` allocations (6 keys + 6 values). Any change
    // that switches to a `nom`/`regex` parser or starts validating
    // values inline will trip this gate.
    let budget = Duration::from_micros(100);
    assert!(
        median < budget,
        "parse_mta_sts_policy median {median:?} exceeded {budget:?}"
    );
}

// ===== TLSRPT record validator =====
//
// `validate_tlsrpt_record` runs once per `_smtp._tls.<domain>` TXT
// record returned during a check. Pure double `contains()`. Below the
// timer floor on its own (<100 ns), so we batch 100× per sample.
#[test]
fn validate_tlsrpt_record_x100_under_budget() {
    let record = "v=TLSRPTv1; rua=mailto:tlsreports@example.com";
    let median = time_median(|| {
        for _ in 0..100 {
            let _ = validate_tlsrpt_record(record);
        }
    });
    // Budget: 1 ms (~25× headroom). Observed P95 (dev): ~41 µs for 100
    // calls (≈ 400 ns each — two `&str::contains` calls per invocation,
    // which scan the record body twice). Single-call cost (<100 ns) is
    // below the timer floor so we batch 100×.
    let budget = Duration::from_micros(1_000);
    assert!(
        median < budget,
        "validate_tlsrpt_record x100 median {median:?} exceeded {budget:?}"
    );
}

// ===== TLSRPT rua= extractor =====
//
// `extract_tlsrpt_rua` runs once per validated TLSRPT record to surface
// the list of reporting URIs for the operator. Realistic records carry
// 1-3 comma-separated URIs in `rua=`.
#[test]
fn extract_tlsrpt_rua_multiple_under_budget() {
    let record = "v=TLSRPTv1; rua=mailto:reports@example.com,https://tlsrpt.example.com/api/v1,mailto:fallback@example.org";
    let median = time_median(|| {
        let _ = extract_tlsrpt_rua(record);
    });
    // Budget: 30 µs (~20× headroom). Observed P95 (dev): ~1.3 µs. Splits
    // on `;`, finds the `rua=` field, splits that on `,`, trims +
    // allocates each URI. Any change that introduces URL validation
    // or escapes-handling inline will trip this gate.
    let budget = Duration::from_micros(30);
    assert!(
        median < budget,
        "extract_tlsrpt_rua median {median:?} exceeded {budget:?}"
    );
}

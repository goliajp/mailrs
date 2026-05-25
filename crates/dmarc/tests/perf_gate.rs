//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_dmarc::{DmarcResultRecord, extract_rua_from_dmarc_record, generate_dmarc_report_xml};

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

fn sample_results(n: usize) -> Vec<DmarcResultRecord> {
    (0..n)
        .map(|i| {
            DmarcResultRecord::new(
                format!("192.0.2.{}", i % 255),
                format!("sender{}.example.com", i % 10),
                if i % 3 == 0 { "fail" } else { "pass" },
                if i % 4 == 0 { "fail" } else { "pass" },
                "pass",
                "none",
            )
        })
        .collect()
}

#[test]
fn generate_dmarc_report_xml_n500_under_budget() {
    let results = sample_results(500);
    let median = time_median(|| {
        let _ = generate_dmarc_report_xml("Org", "a@x", "r", "example.com", 0, 86400, &results);
    });
    // Budget: 30 ms. Observed P95: ~1.5 ms.
    let budget = Duration::from_millis(30);
    assert!(
        median < budget,
        "generate_dmarc_report_xml(500) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn extract_rua_under_budget() {
    let typical = "v=DMARC1; p=quarantine; rua=mailto:dmarc@example.com; ruf=mailto:forensic@example.com; fo=1; pct=100; adkim=r; aspf=r";
    let median = time_median(|| {
        let _ = extract_rua_from_dmarc_record(typical);
    });
    // Budget: 50 µs. Observed P95: ~1 µs.
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "extract_rua median {median:?} exceeded {budget:?}"
    );
}

//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_intelligence::importance::{ImportanceSignals, calculate_importance};
use mailrs_intelligence::structured::extract_structured_data;

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

const LONG_HTML: &str = r#"<html><head>
<script type="application/ld+json">
{"@context":"http://schema.org","@type":"FlightReservation","reservationId":"RXJ34P","reservationFor":{"@type":"Flight","flightNumber":"110","airline":{"@type":"Airline","name":"United Airlines","iataCode":"UA"}}}
</script>
</head><body><h1>Your flight is confirmed</h1>
<script type="application/ld+json">
{"@type":"Order","orderNumber":"W001","seller":{"name":"Acme"},"totalPrice":"39.98","priceCurrency":"USD"}
</script>
</body></html>"#;

#[test]
fn extract_structured_long_under_budget() {
    let median = time_median(|| {
        let _ = extract_structured_data(LONG_HTML);
    });
    // Budget: 5 ms. Observed P95: ~200 µs (JSON-LD parse over two nested blocks).
    let budget = Duration::from_millis(5);
    assert!(
        median < budget,
        "extract_structured_data median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn calculate_importance_under_budget() {
    let signals = ImportanceSignals {
        is_mutual_contact: true,
        is_direct_recipient: true,
        is_reply_to_my_email: false,
        has_action_items: true,
        is_vip_sender: false,
        is_bulk_sender: false,
        is_mailing_list: false,
        is_automated: false,
        has_tracking_pixel: false,
        is_template_heavy: false,
        text_to_html_ratio: 0.8,
        link_count: 3,
        contact_importance_bias: 0.0,
    };
    let median = time_median(|| {
        let _ = calculate_importance(&signals);
    });
    // Budget: 50 µs. Observed P95: ~500 ns (pure arithmetic).
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "calculate_importance median {median:?} exceeded {budget:?}"
    );
}

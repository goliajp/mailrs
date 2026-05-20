//! Micro-benchmarks for the hot paths in mailrs-intelligence.
//!
//! Run with: `cargo bench -p mailrs-intelligence`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_intelligence::importance::{ImportanceSignals, calculate_importance};
use mailrs_intelligence::structured::extract_structured_data;

const SHORT_HTML: &str = r#"<html><body>
<script type="application/ld+json">
{"@type":"Event","name":"Standup","startDate":"2026-01-01T09:00:00"}
</script>
</body></html>"#;

const LONG_HTML: &str = r#"<html><head>
<style>body { color: red; }</style>
<script type="application/ld+json">
{
  "@context": "http://schema.org",
  "@type": "FlightReservation",
  "reservationId": "RXJ34P",
  "reservationStatus": "http://schema.org/ReservationConfirmed",
  "reservationFor": {
    "@type": "Flight",
    "flightNumber": "110",
    "airline": {"@type": "Airline", "name": "United Airlines", "iataCode": "UA"},
    "departureAirport": {"@type": "Airport", "name": "San Francisco", "iataCode": "SFO"},
    "arrivalAirport": {"@type": "Airport", "name": "Los Angeles", "iataCode": "LAX"},
    "departureTime": "2026-04-01T08:00:00-07:00",
    "arrivalTime": "2026-04-01T09:30:00-07:00"
  },
  "potentialAction": {
    "@type": "ViewAction",
    "name": "View Booking",
    "target": {"urlTemplate": "https://example.com/booking/RXJ34P"}
  }
}
</script>
</head><body><h1>Your flight is confirmed</h1>
<script type="application/ld+json">
{"@type":"Order","orderNumber":"W001","seller":{"name":"Acme"},"orderedItem":[
  {"@type":"Product","name":"Widget","orderQuantity":2,"price":"19.99"}
],"totalPrice":"39.98","priceCurrency":"USD"}
</script>
<p>Lots of html content here. </p>
</body></html>"#;

fn bench_extract_structured(c: &mut Criterion) {
    let mut group = c.benchmark_group("extract_structured_data");
    group.bench_function("short_single_event", |b| {
        b.iter(|| extract_structured_data(black_box(SHORT_HTML)))
    });
    group.bench_function("long_with_flight_and_order", |b| {
        b.iter(|| extract_structured_data(black_box(LONG_HTML)))
    });
    group.finish();
}

fn bench_importance(c: &mut Criterion) {
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
    c.bench_function("calculate_importance", |b| {
        b.iter(|| calculate_importance(black_box(&signals)))
    });
}

criterion_group!(benches, bench_extract_structured, bench_importance);
criterion_main!(benches);

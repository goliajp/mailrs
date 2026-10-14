//! A letter dated a month after it arrived, so the list keeps it on top.
//!
//! Reported 2026-10-15: `base2026@tintgate.com`, received 2026-10-12,
//! `Date: Thu, 12 Nov 2026`.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "dated-in-the-future";
const DAY: i64 = 86_400;

fn findings(date_ahead: i64) -> mailrs_fraud::Findings {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let facts = Facts {
        from: "base2026 <base2026@tintgate.com>",
        subject: "邀请使用 TinyGate 高性能 API 网关",
        date_ahead,
        ..Facts::default()
    };
    let out = rules.classify(&facts, &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
}

#[test]
fn the_reported_letter_is_held() {
    let f = findings(31 * DAY - 1);
    assert!(f.has(RULE));
    assert!(f.hold_worthy());
}

// adcogov.org's county notice arrived seven hours "early": a zone
// written wrong, not a letter trying to stay on top
#[test]
fn a_clock_or_zone_that_is_off_by_hours_is_not() {
    assert!(!findings(7 * 3600).has(RULE));
}

// the widest honest mistake: local time in UTC+14 labelled UTC-12
#[test]
fn the_widest_zone_mistake_is_not() {
    assert!(!findings(26 * 3600).has(RULE));
    assert!(!findings(2 * DAY).has(RULE));
}

#[test]
fn no_dates_to_compare_is_not() {
    assert!(!findings(0).has(RULE));
}

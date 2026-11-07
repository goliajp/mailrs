//! The cases for the QR rules.

use mailrs_fraud::Facts;
use mailrs_fraud::qr::QrFacts;

pub(super) fn cases(base: Facts<'static>) -> Vec<super::Case> {
    vec![
        (
            "qr-text-says-another-domain",
            Facts {
                qr: QrFacts {
                    codes: 1,
                    host_not_in_message: true,
                    text_mismatch: true,
                },
                qr_domains: "pay-here.example",
                has_links: true,
                ..base.clone()
            },
            &[("domains", "pay-here.example")],
        ),
        (
            "qr-only-way-out",
            Facts {
                qr: QrFacts {
                    codes: 1,
                    host_not_in_message: true,
                    text_mismatch: false,
                },
                qr_domains: "pay-here.example",
                ..base.clone()
            },
            &[("domains", "pay-here.example")],
        ),
        (
            "qr-to-a-new-domain",
            Facts {
                qr: QrFacts {
                    codes: 1,
                    host_not_in_message: true,
                    text_mismatch: false,
                },
                qr_domains: "fresh.example",
                new_qr_domain: Some("fresh.example".into()),
                has_links: true,
                ..base.clone()
            },
            &[("domain", "fresh.example")],
        ),
    ]
}

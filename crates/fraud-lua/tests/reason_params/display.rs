//! The case for the rule on our own domain worn in a display name.

use mailrs_fraud::Facts;

pub(super) fn cases(base: Facts<'static>) -> Vec<super::Case> {
    vec![(
        "display-names-our-domain",
        Facts {
            from: "Mail Delivery System - golia.jp <jade@sender.example>",
            ..base
        },
        &[("claimed", "golia.jp"), ("domain", "sender.example")],
    )]
}

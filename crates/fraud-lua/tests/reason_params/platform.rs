//! The case for the platform-mail shortener rule.

use mailrs_fraud::Facts;

pub(super) fn cases(base: Facts<'static>) -> Vec<super::Case> {
    let hosts: &'static [String] = Box::leak(Box::new(["share.google".to_string()]));
    vec![(
        "platform-mail-links-through-a-shortener",
        Facts {
            from: "Someone <noreply@appsheet.com>",
            dkim_signed_by_from_org: true,
            has_links: true,
            link_hosts: hosts,
            ..base
        },
        &[
            ("address", "noreply@appsheet.com"),
            ("shortener", "share.google"),
        ],
    )]
}

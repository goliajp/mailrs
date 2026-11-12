//! The cases for the rules on where links go.

use mailrs_fraud::Facts;
use mailrs_fraud::link_address::LinkAddress;

pub(super) fn cases(base: Facts<'static>) -> Vec<super::Case> {
    vec![
        (
            "link-shows-our-address",
            Facts {
                shown_links: "golia.jp login.evil.example\n",
                ..base.clone()
            },
            &[("shown", "golia.jp"), ("target", "login.evil.example")],
        ),
        (
            "link-forwards-your-address",
            Facts {
                link_address: LinkAddress {
                    base64_via_redirect: Some("smartechla.com".into()),
                    ..LinkAddress::default()
                },
                ..base.clone()
            },
            &[("domain", "smartechla.com")],
        ),
        (
            "link-fragment-carries-an-address",
            Facts {
                link_address: LinkAddress {
                    fragment_address: Some("nearped.com".into()),
                    ..LinkAddress::default()
                },
                ..base
            },
            &[("domain", "nearped.com")],
        ),
    ]
}

//! The case for the burner-domain outreach rule.

use mailrs_fraud::Facts;
use mailrs_fraud::relation::Relation;

pub(super) fn cases(base: Facts<'static>) -> Vec<super::Case> {
    vec![(
        "burner-domain-outreach",
        Facts {
            hosted_mailbox: true,
            site_elsewhere: true,
            relation: Relation {
                first_contact: true,
                ..Relation::default()
            },
            ..base
        },
        &[("domain", "sender.example")],
    )]
}

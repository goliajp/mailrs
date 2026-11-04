//! The third ground: the reported message is held by a hold rule of the
//! rule set in force when it is reported, judged again then. A rule that
//! has been removed, moved to shadow, or made to score only is not a
//! ground, whatever was stored when the message arrived (the cores' half
//! of that is the contract test of a stored hold).

use mailrs_core_sidestate::families::reports::Outcome;

use super::report;
use super::tests::{held, world};

const HOLDS: &str = "rule('parcel-fee', 'content', 6, true, function(m) \
     if m.subject:find('parcel') then return 'asks for a parcel fee' end end)";
const SCORES: &str = "rule('parcel-fee', 'content', 6, false, function(m) \
     if m.subject:find('parcel') then return 'asks for a parcel fee' end end)";
const SHADOW: &str = "rule('parcel-fee', 'content', 6, false, function(m) \
     if m.subject:find('parcel') then return 'asks for a parcel fee' end end, {shadow = true})";

/// Two readers of the template; `u0` reports `m0` under the rules `rules`.
fn reported(test: &str, rules: &str) -> Outcome {
    let f = world(test, 2);
    f.mail.borrow_mut().rules = rules.to_string();
    let (_, o) = report(&f, "u0@example.com", "m0", false).unwrap().unwrap();
    assert_eq!(held(&f, "u1@example.com"), o.state == "taken_back");
    o
}

fn waits_for_no_ground(o: &Outcome) {
    assert_eq!(
        (o.state.as_str(), o.waits.as_slice()),
        ("waiting", &["no_ground".to_string()][..]),
        "{o:?}"
    );
    assert_eq!((&o.hold_rule, &o.rules_version), (&None, &None));
}

#[test]
fn a_hold_the_rules_in_force_no_longer_make_is_not_a_ground() {
    waits_for_no_ground(&reported("rule-removed", ""));
}

#[test]
fn a_rule_moved_to_shadow_is_not_a_ground() {
    waits_for_no_ground(&reported("rule-shadow", SHADOW));
}

#[test]
fn a_rule_that_only_scores_is_not_a_ground() {
    waits_for_no_ground(&reported("rule-scores", SCORES));
}

#[test]
fn a_message_the_rules_in_force_hold_is_a_ground() {
    let o = reported("rule-holds", HOLDS);
    assert_eq!(o.state, "taken_back", "{o:?}");
    assert_eq!(o.grounds, vec!["hold_rule".to_string()]);
    let version = mailrs_fraud_lua::Rules::compile(HOLDS)
        .unwrap()
        .version()
        .to_string();
    assert_eq!(
        (o.hold_rule.as_deref(), o.rules_version.as_deref()),
        (Some("parcel-fee"), Some(version.as_str()))
    );
}

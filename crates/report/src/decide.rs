//! Whose report may take mail out of other people's inboxes.
//!
//! One report always holds the reporter's own copy. Taking the same
//! template from anyone else needs one of three grounds: the reporter can
//! judge mail for everyone, two different people reported it within a day,
//! or the reported message hits a hold-level rule of the rules in force.
//! Without a ground the other copies wait for an administrator to look at
//! them. Even with a ground, an administrator confirms first when the
//! take-back is large, the sender has a long history with us, or the sender's domain
//! is on the manual allow list: those are where a genuine notice is most
//! likely, and taking one back is worse than missing a scam.

use serde::{Deserialize, Serialize};

/// More inboxes than this in one take-back waits for an administrator.
pub const INBOX_LIMIT: usize = 20;
/// A sender we have heard from for longer than this many days waits for an
/// administrator.
pub const HISTORY_DAYS: i64 = 90;
/// People, not reports, within [`REPORT_WINDOW_SECS`].
pub const PEOPLE_NEEDED: usize = 2;
/// The span two reports must fall in to count together.
pub const REPORT_WINDOW_SECS: i64 = 86_400;

/// A ground for taking mail from other inboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ground {
    Administrator,
    TwoPeople,
    HoldRule,
}

/// A reason the other copies wait for an administrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wait {
    /// None of the grounds holds.
    NoGround,
    ManyInboxes,
    LongHistory,
    AllowListed,
}

/// What is known when a report comes in.
#[derive(Debug, Clone, Default)]
pub struct Situation {
    pub reporter_is_administrator: bool,
    /// The people who reported this template within the window, the
    /// reporter included, each named by whom their mail reaches: two
    /// addresses that alias or forward to one person are one entry.
    pub reporters: Vec<String>,
    /// The reported message hits a rule that holds mail, of the rules in
    /// force (not a rule in shadow, not a screening rule).
    pub hold_rule: bool,
    /// Inboxes other than the reporter's that hold a copy.
    pub inboxes: usize,
    /// Days since we first had mail from the sender's domain; `None` when
    /// not known, which is not counted as a history.
    pub sender_history_days: Option<i64>,
    /// The sender's domain is on the manual allow list.
    pub allow_listed: bool,
}

/// What to do with the other copies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Decision {
    /// No other inbox holds a copy: the reporter's is all there is.
    NoneFound,
    /// Take every other copy back into Review.
    TakeBack { grounds: Vec<Ground> },
    /// Hold the reporter's copy; the rest waits for an administrator.
    Confirm {
        grounds: Vec<Ground>,
        waits: Vec<Wait>,
    },
}

/// How many people the reporters are.
pub fn people(reporters: &[String]) -> usize {
    let mut v: Vec<&str> = reporters.iter().map(|r| r.as_str()).collect();
    v.sort_unstable();
    v.dedup();
    v.len()
}

pub fn decide(s: &Situation) -> Decision {
    let mut grounds = Vec::new();
    if s.reporter_is_administrator {
        grounds.push(Ground::Administrator);
    }
    if people(&s.reporters) >= PEOPLE_NEEDED {
        grounds.push(Ground::TwoPeople);
    }
    if s.hold_rule {
        grounds.push(Ground::HoldRule);
    }
    if s.inboxes == 0 {
        return Decision::NoneFound;
    }
    let mut waits = Vec::new();
    if grounds.is_empty() {
        waits.push(Wait::NoGround);
    }
    if s.inboxes > INBOX_LIMIT {
        waits.push(Wait::ManyInboxes);
    }
    if s.sender_history_days.is_some_and(|d| d > HISTORY_DAYS) {
        waits.push(Wait::LongHistory);
    }
    if s.allow_listed {
        waits.push(Wait::AllowListed);
    }
    match waits.is_empty() {
        true => Decision::TakeBack { grounds },
        false => Decision::Confirm { grounds, waits },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(f: impl FnOnce(&mut Situation)) -> Situation {
        let mut s = Situation {
            reporters: vec!["alice".into()],
            inboxes: 3,
            ..Default::default()
        };
        f(&mut s);
        s
    }

    #[test]
    fn one_person_alone_waits_for_an_administrator() {
        assert_eq!(
            decide(&s(|_| {})),
            Decision::Confirm {
                grounds: vec![],
                waits: vec![Wait::NoGround]
            }
        );
    }

    #[test]
    fn each_ground_takes_the_other_copies_back() {
        let admin = decide(&s(|s| s.reporter_is_administrator = true));
        assert_eq!(
            admin,
            Decision::TakeBack {
                grounds: vec![Ground::Administrator]
            }
        );
        let two = decide(&s(|s| s.reporters.push("bob".into())));
        assert_eq!(
            two,
            Decision::TakeBack {
                grounds: vec![Ground::TwoPeople]
            }
        );
        let rule = decide(&s(|s| s.hold_rule = true));
        assert_eq!(
            rule,
            Decision::TakeBack {
                grounds: vec![Ground::HoldRule]
            }
        );
    }

    #[test]
    fn two_addresses_of_one_person_are_one_reporter() {
        // alias and forward resolved by the caller to the same person
        let d = decide(&s(|s| s.reporters.push("alice".into())));
        assert!(
            matches!(d, Decision::Confirm { ref waits, .. } if waits == &[Wait::NoGround]),
            "{d:?}"
        );
    }

    #[test]
    fn each_wait_sends_it_to_an_administrator() {
        let many = decide(&s(|s| {
            s.hold_rule = true;
            s.inboxes = INBOX_LIMIT + 1;
        }));
        let old = decide(&s(|s| {
            s.hold_rule = true;
            s.sender_history_days = Some(HISTORY_DAYS + 1);
        }));
        let allowed = decide(&s(|s| {
            s.hold_rule = true;
            s.allow_listed = true;
        }));
        let waits = |d: Decision| match d {
            Decision::Confirm { waits, .. } => waits,
            other => panic!("expected an administrator to confirm, got {other:?}"),
        };
        assert_eq!(waits(many), vec![Wait::ManyInboxes]);
        assert_eq!(waits(old), vec![Wait::LongHistory]);
        assert_eq!(waits(allowed), vec![Wait::AllowListed]);
    }

    #[test]
    fn twenty_inboxes_and_an_unknown_history_do_not_wait() {
        let d = decide(&s(|s| {
            s.hold_rule = true;
            s.inboxes = INBOX_LIMIT;
            s.sender_history_days = None;
        }));
        assert_eq!(
            d,
            Decision::TakeBack {
                grounds: vec![Ground::HoldRule]
            }
        );
    }

    #[test]
    fn no_other_copy_is_none_found_even_with_a_ground() {
        let d = decide(&s(|s| {
            s.reporter_is_administrator = true;
            s.inboxes = 0;
        }));
        assert_eq!(d, Decision::NoneFound);
    }
}

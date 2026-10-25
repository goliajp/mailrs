//! The behaviour every [`Ops`] implementation must show under the policy.
//! Each scenario starts from an empty store.

use std::net::IpAddr;

use crate::alerts::{self, Kind};

use super::{
    LoginPolicy, Ops, Scopes, Verdict, check, is_familiar, record_failure, record_success,
};

fn ip(s: &str) -> IpAddr {
    s.parse().expect("ip")
}

/// Run every scenario against stores made by `fresh`. `advance` is
/// called with the new clock whenever a scenario moves time; a store
/// that reads a real clock may ignore it only if the scenarios'
/// clocks are real.
pub fn check_ops<O: Ops + crate::events::EventOps>(
    mut fresh: impl FnMut() -> O,
    mut advance: impl FnMut(&mut O, u64),
) {
    let p = LoginPolicy::default();
    let t = 10_000_000;
    locks_by_source_and_account(&mut fresh, &mut advance, &p, t);
    networks_and_sources(&mut fresh, &mut advance, &p, t);
    the_global_count_and_lock_lengths(&mut fresh, &mut advance, &p, t);
    successes_and_familiar_sources(&mut fresh, &mut advance, &p, t);
}

fn locks_by_source_and_account<O: Ops + crate::events::EventOps>(
    fresh: &mut impl FnMut() -> O,
    advance: &mut impl FnMut(&mut O, u64),
    p: &LoginPolicy,
    t: u64,
) {
    // source + account: the fifth failure locks, and the right
    // password is not even looked at while it lasts
    let mut o = fresh();
    advance(&mut o, t);
    for _ in 0..4 {
        record_failure(&mut o, p, ip("198.51.100.7"), "alice@x.test", t).unwrap();
    }
    assert_eq!(
        check(&mut o, p, ip("198.51.100.7"), "alice@x.test", t).unwrap(),
        Verdict::Allowed
    );
    record_failure(&mut o, p, ip("198.51.100.7"), "alice@x.test", t).unwrap();
    assert!(matches!(
        check(&mut o, p, ip("198.51.100.7"), "Alice@X.test", t).unwrap(),
        Verdict::Locked { retry_after_secs } if retry_after_secs > 1700 && retry_after_secs <= 1800
    ));
    // a different source is not affected by that scope
    assert_eq!(
        check(&mut o, p, ip("198.51.100.8"), "alice@x.test", t).unwrap(),
        Verdict::Allowed
    );
    assert!(
        alerts::take(&mut o, 100).unwrap().is_empty(),
        "one source against one account raises no alert"
    );

    // account scope: ten sources, one failure each, lock the account
    // for unfamiliar sources but not for a familiar one
    let mut o = fresh();
    advance(&mut o, t);
    record_success(&mut o, p, ip("203.0.113.9"), "bob@x.test", t).unwrap();
    for i in 0..10 {
        record_failure(&mut o, p, ip(&format!("192.0.2.{i}")), "bob@x.test", t).unwrap();
    }
    assert!(matches!(
        check(&mut o, p, ip("192.0.2.200"), "bob@x.test", t).unwrap(),
        Verdict::Locked { retry_after_secs } if retry_after_secs <= 900
    ));
    assert_eq!(
        check(&mut o, p, ip("203.0.113.77"), "bob@x.test", t).unwrap(),
        Verdict::Allowed,
        "same /24 as a past sign-in"
    );
    assert_eq!(
        check(&mut o, p, ip("192.0.2.200"), "carol@x.test", t).unwrap(),
        Verdict::Allowed,
        "other accounts"
    );
    let raised: Vec<(Kind, String)> = alerts::take(&mut o, 100)
        .unwrap()
        .into_iter()
        .map(|a| (a.kind, a.subject))
        .collect();
    assert!(
        raised.contains(&(Kind::AccountLocked, "bob@x.test".into())),
        "the account lock raised an alert: {raised:?}"
    );
}

fn networks_and_sources<O: Ops + crate::events::EventOps>(
    fresh: &mut impl FnMut() -> O,
    advance: &mut impl FnMut(&mut O, u64),
    p: &LoginPolicy,
    t: u64,
) {
    // one account signing in from three networks in a few minutes
    let mut o = fresh();
    advance(&mut o, t);
    for net in ["198.51.100.1", "203.0.113.1", "192.0.2.1"] {
        record_success(&mut o, p, ip(net), "kim@x.test", t).unwrap();
    }
    let raised: Vec<Kind> = alerts::take(&mut o, 100)
        .unwrap()
        .into_iter()
        .map(|a| a.kind)
        .collect();
    assert_eq!(raised, vec![Kind::ManyNetworks]);
    record_success(&mut o, p, ip("198.18.0.1"), "kim@x.test", t).unwrap();
    assert!(
        alerts::take(&mut o, 100).unwrap().is_empty(),
        "once an hour"
    );

    // source scope: twenty accounts from one address lock the address,
    // except for an account that knows it
    let mut o = fresh();
    advance(&mut o, t);
    record_success(&mut o, p, ip("198.51.100.20"), "dave@x.test", t).unwrap();
    for i in 0..20 {
        record_failure(&mut o, p, ip("198.51.100.20"), &format!("u{i}@x.test"), t).unwrap();
    }
    assert!(matches!(
        check(&mut o, p, ip("198.51.100.20"), "erin@x.test", t).unwrap(),
        Verdict::Locked { .. }
    ));
    assert_eq!(
        check(&mut o, p, ip("198.51.100.20"), "dave@x.test", t).unwrap(),
        Verdict::Allowed
    );
    // IPv6 counts by /64
    let mut o = fresh();
    advance(&mut o, t);
    for i in 0..20 {
        record_failure(
            &mut o,
            p,
            ip(&format!("2001:db8:1:2::{i:x}")),
            &format!("v{i}@x.test"),
            t,
        )
        .unwrap();
    }
    assert!(matches!(
        check(&mut o, p, ip("2001:db8:1:2:ffff::1"), "w@x.test", t).unwrap(),
        Verdict::Locked { .. }
    ));
    assert_eq!(
        check(&mut o, p, ip("2001:db8:1:3::1"), "w@x.test", t).unwrap(),
        Verdict::Allowed
    );
}

fn the_global_count_and_lock_lengths<O: Ops + crate::events::EventOps>(
    fresh: &mut impl FnMut() -> O,
    advance: &mut impl FnMut(&mut O, u64),
    p: &LoginPolicy,
    t: u64,
) {
    // global: a hundred failures spread over sources and accounts shut
    // unfamiliar sources out, familiar ones still get in, and the door
    // opens again once the window has moved on
    let mut o = fresh();
    advance(&mut o, t);
    record_success(&mut o, p, ip("203.0.113.5"), "frank@x.test", t).unwrap();
    for i in 0..100u32 {
        let src = format!("10.{}.{}.1", i / 200, i % 200);
        record_failure(&mut o, p, ip(&src), &format!("g{}@x.test", i % 7), t).unwrap();
    }
    assert!(matches!(
        check(&mut o, p, ip("192.0.2.99"), "new@x.test", t).unwrap(),
        Verdict::Locked { .. }
    ));
    assert_eq!(
        check(&mut o, p, ip("203.0.113.5"), "frank@x.test", t).unwrap(),
        Verdict::Allowed
    );
    let later = t + p.global_window_secs + 60;
    advance(&mut o, later);
    assert_eq!(
        check(&mut o, p, ip("192.0.2.99"), "new@x.test", later).unwrap(),
        Verdict::Allowed
    );

    // a lock ends, a second one in the same day is twice as long, and
    // failures spread out beyond the window never add up
    let mut o = fresh();
    advance(&mut o, t);
    for _ in 0..5 {
        record_failure(&mut o, p, ip("198.51.100.30"), "gina@x.test", t).unwrap();
    }
    let after = t + 1801;
    advance(&mut o, after);
    assert_eq!(
        check(&mut o, p, ip("198.51.100.30"), "gina@x.test", after).unwrap(),
        Verdict::Allowed
    );
    for _ in 0..5 {
        record_failure(&mut o, p, ip("198.51.100.30"), "gina@x.test", after).unwrap();
    }
    assert!(matches!(
        check(&mut o, p, ip("198.51.100.30"), "gina@x.test", after).unwrap(),
        Verdict::Locked { retry_after_secs } if retry_after_secs > 1800
    ));
    let mut o = fresh();
    let mut now = t;
    for _ in 0..12 {
        advance(&mut o, now);
        record_failure(&mut o, p, ip("198.51.100.40"), "hank@x.test", now).unwrap();
        now += 1000;
    }
    advance(&mut o, now);
    assert_eq!(
        check(&mut o, p, ip("198.51.100.40"), "hank@x.test", now).unwrap(),
        Verdict::Allowed
    );
}

fn successes_and_familiar_sources<O: Ops + crate::events::EventOps>(
    fresh: &mut impl FnMut() -> O,
    advance: &mut impl FnMut(&mut O, u64),
    p: &LoginPolicy,
    t: u64,
) {
    // a success clears the source + account count
    let mut o = fresh();
    advance(&mut o, t);
    for _ in 0..4 {
        record_failure(&mut o, p, ip("198.51.100.50"), "ida@x.test", t).unwrap();
    }
    record_success(&mut o, p, ip("198.51.100.50"), "ida@x.test", t).unwrap();
    record_failure(&mut o, p, ip("198.51.100.50"), "ida@x.test", t).unwrap();
    assert_eq!(
        check(&mut o, p, ip("198.51.100.50"), "ida@x.test", t).unwrap(),
        Verdict::Allowed
    );

    // familiar sources expire after their window, and at most
    // `familiar_max` are kept, the newest
    let mut o = fresh();
    advance(&mut o, t);
    for i in 0..=p.familiar_max {
        record_success(
            &mut o,
            p,
            ip(&format!("198.18.{i}.1")),
            "jo@x.test",
            t + i as u64,
        )
        .unwrap();
    }
    let s = Scopes::of(ip("198.18.0.1"), "jo@x.test");
    assert_eq!(o.hgetall(&s.familiar_key).unwrap().len(), p.familiar_max);
    assert!(
        !is_familiar(&mut o, p, &s, t + 100).unwrap(),
        "the oldest was dropped"
    );
    let s = Scopes::of(ip("198.18.1.1"), "jo@x.test");
    assert!(is_familiar(&mut o, p, &s, t + 100).unwrap());
    assert!(!is_familiar(&mut o, p, &s, t + p.familiar_secs + 1).unwrap());
}

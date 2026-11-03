use super::*;

const DAY: i64 = 86_400;

fn conn(name: &str) -> kevy_client::Connection {
    kevy_client::Connection::connect(&format!("mem://domain-first-{name}")).unwrap()
}

fn hosts(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_first_seen_time_is_written_once() {
    let mut c = conn("once");
    record(
        &mut c,
        &hosts(&["mail.shop.example", "cdn.shop.example"]),
        100,
    )
    .unwrap();
    record(&mut c, &hosts(&["other.shop.example"]), 999).unwrap();
    assert_eq!(first_seen(&mut c, "shop.example"), Some(100));
    assert_eq!(since(&mut c), Some(100));
}

#[test]
fn a_message_records_at_most_fifty_link_domains() {
    let mut c = conn("cap");
    let mut many = vec!["sender.example".to_string()];
    many.extend((0..80).map(|i| format!("l{i}.example")));
    record(&mut c, &many, 100).unwrap();
    assert_eq!(census(&mut c, 100).unwrap().keys, 51);
}

#[test]
fn a_domain_mail_came_from_before_recording_is_not_new() {
    let mut c = conn("old");
    c.set(
        mailrs_fraud::brand::seen_key("old.example").as_bytes(),
        b"7",
    )
    .unwrap();
    record(&mut c, &hosts(&["unrelated.example"]), 0).unwrap();
    let n = novelty(Some(&mut c), "old.example", &[], 30 * DAY);
    assert!(!n.sender_new);
    let n = novelty(Some(&mut c), "never.example", &[], 30 * DAY);
    assert!(n.sender_new, "nothing has ever come from it");
}

#[test]
fn a_new_link_domain_is_named_and_an_old_one_is_not() {
    let mut c = conn("links");
    record(&mut c, &hosts(&["s.example", "old-link.example"]), 0).unwrap();
    record(&mut c, &hosts(&["s.example", "new-link.example"]), 20 * DAY).unwrap();
    let n = novelty(
        Some(&mut c),
        "s.example",
        &hosts(&["https-host.old-link.example", "x.new-link.example"]),
        21 * DAY,
    );
    assert_eq!(n.new_link_domain.as_deref(), Some("new-link.example"));
    assert!(!n.sender_new);
    assert_eq!(n.sender_age_days, Some(21));
}

#[test]
fn the_census_counts_the_last_day() {
    let mut c = conn("census");
    record(&mut c, &hosts(&["a.example"]), 0).unwrap();
    record(&mut c, &hosts(&["b.example"]), 10 * DAY).unwrap();
    assert_eq!(
        census(&mut c, 10 * DAY + 60).unwrap(),
        Census {
            keys: 2,
            added_24h: 1
        }
    );
}

#[test]
fn pruning_takes_only_old_domains_seen_once_and_idles_without_writing() {
    let mut c = conn("prune");
    record(&mut c, &hosts(&["once.example"]), 0).unwrap();
    record(&mut c, &hosts(&["often.example"]), 0).unwrap();
    record(&mut c, &hosts(&["linked.example"]), 0).unwrap();
    record(&mut c, &hosts(&["recent.example"]), 170 * DAY).unwrap();
    c.set(
        mailrs_fraud::brand::seen_key("once.example").as_bytes(),
        b"1",
    )
    .unwrap();
    c.set(
        mailrs_fraud::brand::seen_key("often.example").as_bytes(),
        b"5",
    )
    .unwrap();
    let now = 200 * DAY;
    assert_eq!(prune(&mut c, now).unwrap(), 2, "once and linked");
    assert_eq!(first_seen(&mut c, "once.example"), None);
    assert_eq!(first_seen(&mut c, "linked.example"), None);
    assert_eq!(
        c.get(mailrs_fraud::brand::seen_key("once.example").as_bytes())
            .unwrap(),
        None
    );
    assert_eq!(first_seen(&mut c, "often.example"), Some(0));
    assert_eq!(first_seen(&mut c, "recent.example"), Some(170 * DAY));
    assert_eq!(prune(&mut c, now).unwrap(), 0, "nothing left to do");
}

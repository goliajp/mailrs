use super::*;

fn conn(name: &str) -> kevy_client::Connection {
    kevy_client::Connection::connect(&format!("mem://sender-rules-{name}")).unwrap()
}

#[test]
fn a_rule_is_keyed_by_the_bare_address_and_retention_tracks_the_user() {
    let mut c = conn("keyed");
    let rule = SenderRule {
        bucket: Some("promotions".into()),
        keep_latest: Some(3),
        source: "settings".into(),
        ..Default::default()
    };
    put(&mut c, "Me@X", "Shop <NEWS@shop.example>", &rule).unwrap();
    assert_eq!(
        get(&mut c, "me@x", "news@shop.example").unwrap(),
        Some(rule.clone())
    );
    assert_eq!(
        list(&mut c, "me@x").unwrap(),
        vec![("news@shop.example".to_string(), rule)]
    );
    assert_eq!(retaining_users(&mut c).unwrap(), vec!["me@x"]);

    let plain = SenderRule {
        bucket: Some("inbox".into()),
        ..Default::default()
    };
    put(&mut c, "me@x", "news@shop.example", &plain).unwrap();
    assert!(
        retaining_users(&mut c).unwrap().is_empty(),
        "no retention left"
    );
    assert!(delete(&mut c, "me@x", "news@shop.example").unwrap());
    assert!(!delete(&mut c, "me@x", "news@shop.example").unwrap());
}

#[test]
fn an_undone_correction_is_no_longer_open() {
    let mut c = conn("undo");
    let moved = Correction::Moved {
        id: "c1".into(),
        sender: "news@shop.example".into(),
        to_bucket: "promotions".into(),
        moved: vec![Moved {
            thread_id: "t1".into(),
            from_bucket: "inbox".into(),
        }],
        at: 1,
    };
    record(&mut c, "me@x", &moved).unwrap();
    assert_eq!(open_correction(&mut c, "me@x", "c1").unwrap(), Some(moved));
    record(
        &mut c,
        "me@x",
        &Correction::Undone {
            id: "c1".into(),
            at: 2,
        },
    )
    .unwrap();
    assert_eq!(open_correction(&mut c, "me@x", "c1").unwrap(), None);
    assert_eq!(open_correction(&mut c, "me@x", "nope").unwrap(), None);
}

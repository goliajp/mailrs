use super::*;

fn web(n: u32, created_at: i64) -> WebSubscription {
    WebSubscription {
        endpoint: format!("https://fcm.googleapis.com/fcm/send/{n}"),
        p256dh: "k".into(),
        auth: "a".into(),
        created_at,
    }
}

fn phone(n: u32, created_at: i64) -> Device {
    Device {
        endpoint: format!("https://ntfy.example.org/up{n}"),
        p256dh: "k".into(),
        auth: "a".into(),
        name: format!("Phone {n}"),
        created_at,
    }
}

/// Registered by a session that never turned pushes on again.
fn plain(conn: &mut kevy_client::Connection, reg: Registration<'_>, now: i64) -> Registered {
    match register(conn, "me@x.test", reg, Asker::default(), now).unwrap() {
        Filed::Registered(r) => r,
        Filed::Removed => panic!("refused as removed"),
    }
}

fn apns(conn: &mut kevy_client::Connection, token: &str, now: i64) -> Registered {
    let reg = Registration::Apns {
        token,
        platform: "ios",
    };
    plain(conn, reg, now)
}

fn count(conn: &mut kevy_client::Connection) -> usize {
    held(conn, "me@x.test").unwrap().len()
}

/// Ten devices: four iPhones registered at 1–4, three browsers at 5–7 and
/// three Android phones at 8–10.
fn ten(conn: &mut kevy_client::Connection) {
    for (i, token) in ["t1", "t2", "t3", "t4"].into_iter().enumerate() {
        assert!(apns(conn, token, i as i64 + 1).evicted.is_empty());
    }
    for n in 0..3 {
        let sub = web(n, 5 + i64::from(n));
        let r = plain(conn, Registration::Web(&sub), 5 + i64::from(n));
        assert!(r.evicted.is_empty());
    }
    for n in 0..3 {
        let d = phone(n, 8 + i64::from(n));
        let r = plain(conn, Registration::UnifiedPush(&d), 8);
        assert!(r.evicted.is_empty());
    }
    assert_eq!(count(conn), MAX_DEVICES);
}

#[test]
fn the_eleventh_device_on_any_channel_evicts_the_one_least_recently_pushed_to() {
    let mut c = kevy_client::Connection::connect("mem://push-devices-cap").unwrap();
    ten(&mut c);
    // the oldest registration took a push recently, so it is not the one to go
    mark_used(&mut c, "me@x.test", Channel::Apns, "t1", 100).unwrap();

    let eleventh = phone(10, 11);
    let r = plain(&mut c, Registration::UnifiedPush(&eleventh), 11);
    assert_eq!(r.evicted, vec![(Channel::Apns, "t2".to_string())]);
    assert_eq!(r.id, unified_push::id_of(&eleventh.endpoint));
    assert_eq!(count(&mut c), MAX_DEVICES);

    // a browser now: the next never-used device by registration time goes
    let browser = web(9, 12);
    let r = plain(&mut c, Registration::Web(&browser), 12);
    assert_eq!(r.evicted, vec![(Channel::Apns, "t3".to_string())]);

    // used ones outrank never-used ones registered after them
    mark_used(&mut c, "me@x.test", Channel::Apns, "t4", 200).unwrap();
    let first_web = notify::endpoint_field(&web(0, 0).endpoint);
    let r = apns(&mut c, "t9", 13);
    assert_eq!(r.evicted, vec![(Channel::Web, first_web)]);
    assert_eq!(count(&mut c), MAX_DEVICES);
    let tokens: Vec<String> = held(&mut c, "me@x.test")
        .unwrap()
        .into_iter()
        .filter(|d| d.channel == Channel::Apns)
        .map(|d| d.id)
        .collect();
    assert_eq!(tokens.len(), 3);
    for kept in ["t1", "t4", "t9"] {
        assert!(tokens.iter().any(|t| t == kept), "{kept} kept");
    }
}

#[test]
fn registering_a_device_again_counts_it_once() {
    let mut c = kevy_client::Connection::connect("mem://push-devices-again").unwrap();
    ten(&mut c);
    let again = WebSubscription {
        p256dh: "k2".into(),
        ..web(1, 50)
    };
    let r = plain(&mut c, Registration::Web(&again), 50);
    assert!(r.evicted.is_empty(), "the same browser is not an eleventh");
    let r = apns(&mut c, "t2", 51);
    assert!(
        r.evicted.is_empty(),
        "every app launch re-registers its token"
    );
    let d = Device {
        name: "Renamed".into(),
        ..phone(2, 52)
    };
    let r = plain(&mut c, Registration::UnifiedPush(&d), 52);
    assert!(r.evicted.is_empty());
    assert_eq!(count(&mut c), MAX_DEVICES);
}

#[test]
fn last_used_times_of_devices_gone_are_dropped() {
    let mut c = kevy_client::Connection::connect("mem://push-devices-stale").unwrap();
    let d = phone(1, 1);
    let r = plain(&mut c, Registration::UnifiedPush(&d), 1);
    mark_used(&mut c, "me@x.test", Channel::UnifiedPush, &r.id, 5).unwrap();
    mark_used(&mut c, "me@x.test", Channel::Apns, "t1", 5).unwrap();
    // the distributor answered 410 and the sender dropped the phone
    unified_push::remove(&mut c, "me@x.test", &r.id).unwrap();
    apns(&mut c, "t1", 6);
    let used = last_used(&mut c, "me@x.test").unwrap();
    assert_eq!(used.len(), 1);
    assert_eq!(used.get("apns:t1"), Some(&5));
}

#[test]
fn a_device_registered_again_after_its_last_push_counts_as_used_then() {
    let mut c = kevy_client::Connection::connect("mem://push-devices-later").unwrap();
    ten(&mut c);
    // t1 took a push at 20 and its app re-registered at 30; t2 took one at
    // 25 and was not opened since: t2 is the least recently used
    mark_used(&mut c, "me@x.test", Channel::Apns, "t1", 20).unwrap();
    mark_used(&mut c, "me@x.test", Channel::Apns, "t2", 25).unwrap();
    for (token, at) in [("t3", 26), ("t4", 27)] {
        mark_used(&mut c, "me@x.test", Channel::Apns, token, at).unwrap();
    }
    for n in 0..3 {
        let field = notify::endpoint_field(&web(n, 0).endpoint);
        mark_used(&mut c, "me@x.test", Channel::Web, &field, 28).unwrap();
        let id = unified_push::id_of(&phone(n, 0).endpoint);
        mark_used(&mut c, "me@x.test", Channel::UnifiedPush, &id, 28).unwrap();
    }
    assert!(apns(&mut c, "t1", 30).evicted.is_empty());
    let r = apns(&mut c, "t9", 31);
    assert_eq!(r.evicted, vec![(Channel::Apns, "t2".to_string())]);
}

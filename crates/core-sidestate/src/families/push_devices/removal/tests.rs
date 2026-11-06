use super::super::{Asker, Filed, MAX_DEVICES, Registration, held, register};
use super::*;
use crate::families::notify::{self, WebSubscription};
use crate::families::unified_push::{self, Device};

const ME: &str = "me@x.test";

fn phone(n: u32) -> Device {
    Device {
        endpoint: format!("https://ntfy.example.org/up{n}"),
        p256dh: "k".into(),
        auth: "a".into(),
        name: format!("Phone {n}"),
        created_at: i64::from(n),
    }
}

fn browser(n: u32) -> WebSubscription {
    WebSubscription {
        endpoint: format!("https://fcm.googleapis.com/fcm/send/{n}"),
        p256dh: "k".into(),
        auth: "a".into(),
        created_at: i64::from(n),
    }
}

fn by(session: &str) -> Asker<'_> {
    Asker {
        session: Some(session),
        resume: false,
    }
}

fn file(c: &mut kevy_client::Connection, reg: Registration<'_>, who: Asker<'_>) -> Filed {
    register(c, ME, reg, who, 10).unwrap()
}

fn holds(c: &mut kevy_client::Connection, channel: Channel, id: &str) -> bool {
    held(c, ME)
        .unwrap()
        .iter()
        .any(|d| d.channel == channel && d.id == id)
}

#[test]
fn a_device_removed_from_another_device_stays_out_until_its_person_turns_it_on() {
    let mut c = kevy_client::Connection::connect("mem://push-removal-person").unwrap();
    let d = phone(1);
    let id = unified_push::id_of(&d.endpoint);
    file(&mut c, Registration::UnifiedPush(&d), by("s-phone"));

    assert!(remove(&mut c, ME, Channel::UnifiedPush, &id, Some("s-laptop"), 20).unwrap());
    assert!(!holds(&mut c, Channel::UnifiedPush, &id));
    assert_eq!(
        removed(&mut c, ME, Channel::UnifiedPush).unwrap(),
        vec![id.clone()]
    );
    assert!(removed(&mut c, ME, Channel::Apns).unwrap().is_empty());
    let field = format!("unifiedpush:{id}");
    let ttl = c
        .httl(removed_key(ME).as_bytes(), &[field.as_bytes()])
        .unwrap();
    let kept = REMOVED_KEPT.as_secs() as i64;
    assert!(
        ttl[0] > kept - 60 && ttl[0] <= kept,
        "expires in 30 days: {ttl:?}"
    );

    // the phone's own launch-time registration does not undo it
    assert_eq!(
        file(&mut c, Registration::UnifiedPush(&d), by("s-phone")),
        Filed::Removed
    );
    assert!(!holds(&mut c, Channel::UnifiedPush, &id));

    // its person turning pushes on does, and clears the record
    let on = Asker {
        session: Some("s-phone"),
        resume: true,
    };
    assert!(matches!(
        file(&mut c, Registration::UnifiedPush(&d), on),
        Filed::Registered(_)
    ));
    assert!(holds(&mut c, Channel::UnifiedPush, &id));
    assert!(
        removed(&mut c, ME, Channel::UnifiedPush)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn every_channel_keeps_a_removal() {
    let mut c = kevy_client::Connection::connect("mem://push-removal-channels").unwrap();
    let token = Registration::Apns {
        token: "t1",
        platform: "ios",
    };
    let sub = browser(1);
    let web_id = notify::endpoint_field(&sub.endpoint);
    // registered before devices were linked to sessions, or with an agent key
    file(&mut c, token, Asker::default());
    file(&mut c, Registration::Web(&sub), by("s-browser"));
    assert!(remove(&mut c, ME, Channel::Apns, "t1", Some("s-laptop"), 20).unwrap());
    assert!(remove(&mut c, ME, Channel::Web, &web_id, Some("s-laptop"), 20).unwrap());
    assert_eq!(
        removed(&mut c, ME, Channel::Apns).unwrap(),
        vec!["t1".to_string()]
    );
    assert_eq!(removed(&mut c, ME, Channel::Web).unwrap(), vec![web_id]);
    assert_eq!(file(&mut c, token, by("s-phone")), Filed::Removed);
    assert_eq!(
        file(&mut c, Registration::Web(&sub), by("s-browser")),
        Filed::Removed
    );
}

#[test]
fn a_device_removing_itself_keeps_no_record() {
    let mut c = kevy_client::Connection::connect("mem://push-removal-self").unwrap();
    let d = phone(1);
    let id = unified_push::id_of(&d.endpoint);
    file(&mut c, Registration::UnifiedPush(&d), by("s-phone"));
    assert!(remove(&mut c, ME, Channel::UnifiedPush, &id, Some("s-phone"), 20).unwrap());
    assert!(
        removed(&mut c, ME, Channel::UnifiedPush)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        file(&mut c, Registration::UnifiedPush(&d), by("s-phone")),
        Filed::Registered(_)
    ));
    // nothing there, nothing recorded
    assert!(!remove(&mut c, ME, Channel::Apns, "t9", Some("s-laptop"), 20).unwrap());
    assert!(removed(&mut c, ME, Channel::Apns).unwrap().is_empty());
}

#[test]
fn the_cap_and_dead_endpoints_keep_no_record() {
    let mut c = kevy_client::Connection::connect("mem://push-removal-cap").unwrap();
    for n in 0..=MAX_DEVICES as u32 {
        let d = phone(n);
        register(
            &mut c,
            ME,
            Registration::UnifiedPush(&d),
            by("s"),
            i64::from(n),
        )
        .unwrap();
    }
    assert_eq!(held(&mut c, ME).unwrap().len(), MAX_DEVICES);
    // the distributor answered 410
    unified_push::remove(&mut c, ME, &unified_push::id_of(&phone(5).endpoint)).unwrap();
    assert!(
        removed(&mut c, ME, Channel::UnifiedPush)
            .unwrap()
            .is_empty()
    );
    let evicted = phone(0);
    assert!(matches!(
        file(&mut c, Registration::UnifiedPush(&evicted), by("s")),
        Filed::Registered(_)
    ));
}

#[test]
fn ending_a_session_stops_pushes_to_the_devices_it_registered() {
    let mut c = kevy_client::Connection::connect("mem://push-removal-session").unwrap();
    let token = Registration::Apns {
        token: "t1",
        platform: "ios",
    };
    let (sub, d) = (browser(1), phone(1));
    file(&mut c, token, by("s1"));
    file(&mut c, Registration::Web(&sub), by("s2"));
    file(&mut c, Registration::UnifiedPush(&d), by("s1"));
    assert_eq!(drop_for_session(&mut c, ME, "s1").unwrap(), 2);
    let left = held(&mut c, ME).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].channel, Channel::Web);
    for ch in [Channel::Apns, Channel::Web, Channel::UnifiedPush] {
        assert!(
            removed(&mut c, ME, ch).unwrap().is_empty(),
            "no record: {ch:?}"
        );
    }
    let links = c.hgetall(session_link_key(ME).as_bytes()).unwrap();
    assert_eq!(links.len(), 2, "only the browser's link is left");
    // signing in again registers again
    assert!(matches!(
        file(&mut c, token, by("s3")),
        Filed::Registered(_)
    ));
    assert_eq!(drop_for_session(&mut c, ME, "s1").unwrap(), 0);
}

#[test]
fn ending_every_session_stops_every_push() {
    let mut c = kevy_client::Connection::connect("mem://push-removal-all").unwrap();
    let token = Registration::Apns {
        token: "t1",
        platform: "ios",
    };
    let d = phone(1);
    file(&mut c, token, Asker::default());
    file(&mut c, Registration::UnifiedPush(&d), by("s1"));
    super::super::mark_used(&mut c, ME, Channel::Apns, "t1", 11).unwrap();
    assert_eq!(drop_all(&mut c, ME).unwrap(), 2);
    assert!(held(&mut c, ME).unwrap().is_empty());
    for key in [session_link_key(ME), used_key(ME)] {
        assert_eq!(c.exists(&[key.as_bytes()]).unwrap(), 0, "{key}");
    }
}

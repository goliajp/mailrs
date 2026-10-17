use std::path::Path;

use kevy_client::Connection;

use super::*;

fn conn(name: &str) -> Connection {
    Connection::connect(&format!("mem://mock-{name}")).unwrap()
}

fn meta(ns: &str, id: &str, tag: &str, subject: &str) -> MessageMeta {
    MessageMeta {
        id: id.to_string(),
        namespace: ns.to_string(),
        tag: tag.to_string(),
        direction: Direction::In,
        rcpt: format!("{ns}.{tag}@mock.example.test"),
        size: 0,
        received_at: 0,
        summary: mailrs_mock::parsed::Summary {
            subject: subject.to_string(),
            ..Default::default()
        },
    }
}

fn ids(conn: &mut Connection, ns: &str) -> Vec<String> {
    let all = Filter {
        limit: usize::MAX,
        ..Default::default()
    };
    list_messages(conn, ns, &all)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect()
}

fn files(root: &Path, ns: &str) -> usize {
    std::fs::read_dir(root.join(ns))
        .map(|d| d.count())
        .unwrap_or(0)
}

#[test]
fn namespaces_are_global_capped_per_user_and_validated() {
    let mut c = conn("ns");
    create_namespace(&mut c, "a@x.test", "alpha").unwrap();
    assert!(matches!(
        create_namespace(&mut c, "b@x.test", "alpha"),
        Err(MockError::Exists)
    ));
    assert!(matches!(
        create_namespace(&mut c, "a@x.test", "abuse"),
        Err(MockError::BadName(_))
    ));
    for i in 1..MAX_NAMESPACES_PER_USER {
        create_namespace(&mut c, "a@x.test", &format!("alpha{i}")).unwrap();
    }
    assert!(matches!(
        create_namespace(&mut c, "a@x.test", "one-more"),
        Err(MockError::TooManyNamespaces)
    ));
    assert_eq!(list_namespaces(&mut c, Some("A@x.test")).unwrap().len(), 10);
    assert!(
        list_namespaces(&mut c, Some("b@x.test"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn storing_twice_keeps_one_copy_and_the_listing_matches_the_files() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = conn("twice");
    create_namespace(&mut c, "a@x.test", "twice").unwrap();
    let id = spool_message_id("1700000000.M1P2.host", 0);
    assert_eq!(spool_message_id("1700000000.M1P2.host", 0), id);
    assert_ne!(spool_message_id("1700000000.M1P2.host", 1), id);
    let raw = b"Subject: hi\r\n\r\nbody";
    let first = store_message(&mut c, tmp.path(), meta("twice", &id, "t", "hi"), raw).unwrap();
    let again = store_message(&mut c, tmp.path(), meta("twice", &id, "t", "hi"), raw).unwrap();
    assert_eq!((first, again), (Stored::Stored, Stored::AlreadyStored));
    assert_eq!(ids(&mut c, "twice"), std::slice::from_ref(&id));
    assert_eq!(files(tmp.path(), "twice"), 1);
    assert_eq!(total_bytes(&mut c).unwrap(), raw.len() as i64);
    assert_eq!(read_raw(tmp.path(), "twice", &id).unwrap(), raw);
}

#[test]
fn mail_for_a_missing_namespace_is_not_stored() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = conn("missing");
    let got = store_message(&mut c, tmp.path(), meta("nobody", "x1", "t", "s"), b"x").unwrap();
    assert_eq!(got, Stored::NoNamespace);
    assert_eq!(files(tmp.path(), "nobody"), 0);
}

#[test]
fn the_oldest_are_dropped_past_the_cap_and_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = conn("cap");
    create_namespace(&mut c, "a@x.test", "capped").unwrap();
    for i in 0..MAX_MESSAGES + 3 {
        let m = meta("capped", &format!("id{i:04}"), "t", "s");
        store_message(&mut c, tmp.path(), m, b"0123456789").unwrap();
    }
    let kept = ids(&mut c, "capped");
    assert_eq!(kept.len(), MAX_MESSAGES);
    assert!(!kept.contains(&"id0000".to_string()));
    assert!(!kept.contains(&"id0002".to_string()));
    assert!(kept.contains(&"id0003".to_string()));
    assert_eq!(files(tmp.path(), "capped"), MAX_MESSAGES);
    assert_eq!(total_bytes(&mut c).unwrap(), 10 * MAX_MESSAGES as i64);
    let ns = get_namespace(&mut c, "capped").unwrap().unwrap();
    assert_eq!((ns.messages, ns.dropped), (MAX_MESSAGES, 3));
}

#[test]
fn filters_and_deletes_keep_hash_zset_and_files_in_step() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = conn("filters");
    create_namespace(&mut c, "a@x.test", "filt").unwrap();
    for (id, tag, subject) in [
        ("a1", "signup", "Verify"),
        ("a2", "reset", "Reset"),
        ("a3", "signup-2", "Welcome"),
    ] {
        store_message(&mut c, tmp.path(), meta("filt", id, tag, subject), b"x").unwrap();
    }
    let f = |tag: Option<&str>, prefix: Option<&str>, subject: Option<&str>| Filter {
        tag: tag.map(str::to_string),
        tag_prefix: prefix.map(str::to_string),
        subject: subject.map(str::to_string),
        limit: 10,
        ..Default::default()
    };
    let got = |c: &mut Connection, f: Filter| -> Vec<String> {
        list_messages(c, "filt", &f)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect()
    };
    assert_eq!(got(&mut c, f(Some("signup"), None, None)), ["a1"]);
    assert_eq!(got(&mut c, f(None, Some("signup"), None)), ["a3", "a1"]);
    assert_eq!(got(&mut c, f(None, None, Some("reset"))), ["a2"]);
    assert!(get_message(&mut c, "other", "a1").unwrap().is_none());

    assert!(delete_message(&mut c, tmp.path(), "filt", "a2").unwrap());
    assert!(!delete_message(&mut c, tmp.path(), "filt", "a2").unwrap());
    assert_eq!(
        delete_messages(&mut c, tmp.path(), "filt", Some("signup")).unwrap(),
        1
    );
    assert_eq!(ids(&mut c, "filt"), ["a3"]);
    assert_eq!(files(tmp.path(), "filt"), 1);
    assert_eq!(total_bytes(&mut c).unwrap(), 1);

    delete_namespace(&mut c, tmp.path(), "filt").unwrap();
    assert!(!namespace_exists(&mut c, "filt").unwrap());
    assert!(c.get(b"mailrs:mock:msg:a3").unwrap().is_none());
    assert!(!tmp.path().join("filt").exists());
    assert_eq!(total_bytes(&mut c).unwrap(), 0);
    assert!(
        list_namespaces(&mut c, Some("a@x.test"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn since_stops_at_older_mail() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = conn("since");
    create_namespace(&mut c, "a@x.test", "since").unwrap();
    store_message(&mut c, tmp.path(), meta("since", "old", "t", "s"), b"x").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let mark = now_ms();
    store_message(&mut c, tmp.path(), meta("since", "new", "t", "s"), b"x").unwrap();
    let f = Filter {
        since: Some(mark),
        limit: 10,
        ..Default::default()
    };
    let got: Vec<String> = list_messages(&mut c, "since", &f)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(got, ["new"]);
}

#[test]
fn a_sweep_expires_old_mail_and_then_rests() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = conn("sweep");
    create_namespace(&mut c, "a@x.test", "sweep").unwrap();
    store_message(&mut c, tmp.path(), meta("sweep", "s1", "t", "s"), b"abc").unwrap();
    store_message(&mut c, tmp.path(), meta("sweep", "s2", "t", "s"), b"de").unwrap();

    let now = now_ms();
    let quiet = sweep(&mut c, tmp.path(), now).unwrap();
    assert!(!quiet.changed_anything(), "{quiet:?}");

    let later = now + RETENTION.as_millis() as i64 + 1;
    let report = sweep(&mut c, tmp.path(), later).unwrap();
    assert_eq!(report.expired, 2);
    assert!(ids(&mut c, "sweep").is_empty());
    assert_eq!(files(tmp.path(), "sweep"), 0);
    assert_eq!(total_bytes(&mut c).unwrap(), 0);
    assert!(!sweep(&mut c, tmp.path(), later).unwrap().changed_anything());
}

#[test]
fn a_sweep_corrects_the_byte_total_and_removes_unowned_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = conn("bytes");
    create_namespace(&mut c, "a@x.test", "bytes").unwrap();
    store_message(&mut c, tmp.path(), meta("bytes", "b1", "t", "s"), b"abcd").unwrap();
    c.set(b"mailrs:mock:bytes", b"999").unwrap();
    std::fs::create_dir_all(tmp.path().join("gone")).unwrap();
    std::fs::write(tmp.path().join("gone/x.eml"), b"x").unwrap();
    let report = sweep(&mut c, tmp.path(), now_ms()).unwrap();
    assert!(report.bytes_corrected);
    assert_eq!(report.orphan_files, 1);
    assert_eq!(total_bytes(&mut c).unwrap(), 4);
    assert!(!tmp.path().join("gone").exists());
}

#[test]
fn an_smtp_token_verifies_until_it_is_rotated() {
    let mut c = conn("token");
    create_namespace(&mut c, "a@x.test", "tok").unwrap();
    assert!(!verify_smtp_token(&mut c, "tok", "").unwrap());
    let first = rotate_smtp_token(&mut c, "tok").unwrap();
    assert!(verify_smtp_token(&mut c, "tok", &first).unwrap());
    assert!(!verify_smtp_token(&mut c, "tok", "wrong").unwrap());
    assert!(get_namespace(&mut c, "tok").unwrap().unwrap().smtp_enabled);
    let second = rotate_smtp_token(&mut c, "tok").unwrap();
    assert!(!verify_smtp_token(&mut c, "tok", &first).unwrap());
    assert!(verify_smtp_token(&mut c, "tok", &second).unwrap());
    assert!(!verify_smtp_token(&mut c, "other", &second).unwrap());
}

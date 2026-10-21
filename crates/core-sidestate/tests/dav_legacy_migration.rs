//! The old CalDAV / CardDAV keys move into the rows the web reads, once.

use kevy_client::Connection;
use mailrs_core_sidestate::families::{calendar_events, contact_cards, dav_legacy};

const USER: &str = "alice@example.test";
const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:ev-1\r\n\
    DTSTAMP:20260801T000000Z\r\nDTSTART:20260820T090000Z\r\nDTEND:20260820T100000Z\r\n\
    SUMMARY:Phone call\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
const CARD: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob Builder\r\n\
    EMAIL:bob@partner.example\r\nEND:VCARD\r\n";

fn seed(conn: &mut Connection) {
    conn.hset(
        format!("caldav:{USER}:events:default").as_bytes(),
        &[
            (b"ev-1".as_slice(), EVENT.as_bytes()),
            (b"broken".as_slice(), b"not a calendar".as_slice()),
        ],
    )
    .expect("seed events");
    conn.hset(
        format!("carddav:{USER}:contacts:default").as_bytes(),
        &[(b"card-1".as_slice(), CARD.as_bytes())],
    )
    .expect("seed contacts");
}

#[test]
fn migration_moves_old_rows_and_a_second_run_writes_nothing() {
    let mut conn = Connection::connect("mem://dav-legacy-moves").expect("kevy");
    seed(&mut conn);

    let first = dav_legacy::migrate(&mut conn, false).expect("first run");
    assert_eq!(first.events.keys, 1);
    assert_eq!(first.events.walked, 2);
    assert_eq!(first.events.written, 1);
    assert_eq!(first.events.unparseable, 1);
    assert_eq!(first.contacts.walked, 1);
    assert_eq!(first.contacts.written, 1);
    assert_eq!(first.unparseable_samples.len(), 1);

    let ev = calendar_events::get(&mut conn, USER, "ev-1")
        .expect("get")
        .expect("moved");
    assert_eq!(ev.summary, "Phone call");
    assert_eq!(ev.ics.as_deref(), Some(EVENT));
    let listed = calendar_events::list(&mut conn, USER).expect("list");
    assert_eq!(listed.len(), 1, "the moved event is in the index");
    assert_eq!(
        contact_cards::get(&mut conn, USER, "card-1")
            .expect("get")
            .as_deref(),
        Some(CARD)
    );
    let auto = conn
        .hget(
            contact_cards::autocomplete_key(USER).as_bytes(),
            b"bob@partner.example",
        )
        .expect("hget");
    assert_eq!(
        auto.as_deref(),
        Some(&b"Bob Builder <bob@partner.example>"[..])
    );

    let second = dav_legacy::migrate(&mut conn, false).expect("second run");
    assert_eq!(second.events.walked, 2, "the input is still there to walk");
    assert_eq!(second.events.written, 0);
    assert_eq!(second.events.already, 1);
    assert_eq!(second.contacts.written, 0);
    assert_eq!(second.contacts.already, 1);
    assert_eq!(
        second.events.old_keys_deleted, 0,
        "nothing deleted without the flag"
    );
}

/// Something newer under the same uid wins over the old copy.
#[test]
fn an_existing_unified_row_is_kept() {
    let mut conn = Connection::connect("mem://dav-legacy-kept").expect("kevy");
    seed(&mut conn);
    contact_cards::put(&mut conn, USER, "card-1", &CARD.replace("Builder", "Newer"))
        .expect("newer card");

    let r = dav_legacy::migrate(&mut conn, false).expect("run");
    assert_eq!(r.contacts.kept_existing, 1);
    assert_eq!(r.contacts.written, 0);
    let card = contact_cards::get(&mut conn, USER, "card-1")
        .expect("get")
        .expect("card");
    assert!(card.contains("Newer"));
}

/// With the flag, keys that moved cleanly go; a key holding an entry that
/// could not be moved stays, so nothing is lost.
#[test]
fn delete_old_removes_only_clean_keys() {
    let mut conn = Connection::connect("mem://dav-legacy-delete").expect("kevy");
    seed(&mut conn);

    let r = dav_legacy::migrate(&mut conn, true).expect("run");
    assert!(r.delete_old);
    assert_eq!(r.contacts.old_keys_deleted, 1);
    assert_eq!(r.events.old_keys_deleted, 0);
    assert_eq!(
        conn.exists(&[format!("carddav:{USER}:contacts:default").as_bytes()])
            .expect("exists"),
        0
    );
    assert_eq!(
        conn.exists(&[format!("caldav:{USER}:events:default").as_bytes()])
            .expect("exists"),
        1
    );

    let empty = dav_legacy::migrate(&mut conn, true).expect("after");
    assert_eq!(empty.contacts.keys, 0, "no input left to walk");
}

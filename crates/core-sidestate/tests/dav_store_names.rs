//! An event is stored under the `UID` in its text, whatever resource name
//! the client PUT it to, and is still found and listed under that name.

use std::sync::Mutex;

use kevy_client::Connection;
use mailrs_core_sidestate::families::calendar_events::{self, StoredEvent, names};
use mailrs_core_sidestate::families::dav_store::{DavStore, SingleConn};
use mailrs_dav::dispatch::{DavRequest, handle};
use mailrs_dav::{DavError, DavResponse};

const USER: &str = "alice@example.test";

fn ics(uid: &str, summary: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20260801T000000Z\r\n\
         DTSTART:20260901T090000Z\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

struct Fixture {
    kv: Connection,
    store: DavStore<SingleConn>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let url = format!("mem://dav-store-names-{name}");
        Self {
            kv: Connection::connect(&url).expect("kevy"),
            store: DavStore::new(
                USER,
                SingleConn(Mutex::new(Connection::connect(&url).expect("kevy"))),
            ),
        }
    }

    async fn send(&self, method: &str, name: &str, body: &str) -> Result<DavResponse, DavError> {
        let path = match name.is_empty() {
            true => format!("/dav/calendars/{USER}/default/"),
            false => format!("/dav/calendars/{USER}/default/{name}.ics"),
        };
        let req = DavRequest {
            method,
            path: &path,
            depth: Some("1"),
            if_match: None,
            if_none_match: None,
            body,
        };
        handle(&req, USER, &self.store, &self.store).await
    }

    async fn body(&self, method: &str, name: &str, body: &str) -> String {
        let r = self.send(method, name, body).await.expect(method);
        String::from_utf8(r.body).expect("utf8")
    }

    fn uids(&mut self) -> Vec<String> {
        let rows = calendar_events::list(&mut self.kv, USER).expect("list");
        rows.into_iter().map(|e| e.uid).collect()
    }
}

fn invite(uid: &str) -> StoredEvent {
    StoredEvent {
        uid: uid.into(),
        summary: "From mail".into(),
        dtstart: Some("2026-09-01T09:00:00+00:00".into()),
        source: format!("mail:{uid}"),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_client_name_is_kept_but_the_row_is_the_uid() {
    let mut f = Fixture::new("client-name");
    let put = f
        .send("PUT", "phone-a", &ics("meet-1", "Mine"))
        .await
        .expect("put");
    assert_eq!(put.status, 201);
    assert_eq!(f.uids(), ["meet-1"]);

    calendar_events::put(&mut f.kv, USER, &invite("meet-1")).expect("file invite");
    assert_eq!(f.uids(), ["meet-1"], "the invite is the same row");

    let query = "<C:calendar-query xmlns:C=\"urn:ietf:params:xml:ns:caldav\"/>";
    let report = f.body("REPORT", "", query).await;
    assert!(report.contains("/default/phone-a.ics"), "{report}");
    assert!(!report.contains("meet-1.ics"), "{report}");
    let listed = f.body("PROPFIND", "", "").await;
    assert!(listed.contains("/default/phone-a.ics"), "{listed}");

    let got = f.body("GET", "phone-a", "").await;
    assert!(got.contains("SUMMARY:From mail"), "{got}");
    let by_uid = f.send("GET", "meet-1", "").await;
    assert_eq!(by_uid.err(), Some(DavError::NotFound));

    let again = f
        .send("PUT", "phone-a", &ics("meet-1", "Edited"))
        .await
        .expect("update");
    assert_eq!(again.status, 204);

    let deleted = f.send("DELETE", "phone-a", "").await.expect("delete");
    assert_eq!(deleted.status, 204);
    assert!(f.uids().is_empty());
    assert!(names::hrefs(&mut f.kv, USER).expect("hrefs").is_empty());
    let gone = f.send("GET", "phone-a", "").await;
    assert_eq!(gone.err(), Some(DavError::NotFound));
}

#[tokio::test]
async fn a_uid_already_filed_under_another_name_is_refused() {
    let mut f = Fixture::new("uid-conflict");
    f.send("PUT", "phone-a", &ics("meet-1", "Mine"))
        .await
        .expect("put");
    let second = f.send("PUT", "phone-b", &ics("meet-1", "Copy")).await;
    let href = format!("/dav/calendars/{USER}/default/phone-a.ics");
    assert_eq!(second.err(), Some(DavError::UidConflict(href)));

    calendar_events::put(&mut f.kv, USER, &invite("mail-1")).expect("file invite");
    let over_invite = f.send("PUT", "phone-c", &ics("mail-1", "Copy")).await;
    let href = format!("/dav/calendars/{USER}/default/mail-1.ics");
    assert_eq!(over_invite.err(), Some(DavError::UidConflict(href)));
    let at_its_href = f
        .send("PUT", "mail-1", &ics("mail-1", "Accepted"))
        .await
        .expect("put at the invite's own href");
    assert_eq!(at_its_href.status, 204);

    let other_uid = f.send("PUT", "phone-a", &ics("meet-2", "Swap")).await;
    assert_eq!(other_uid.err(), Some(DavError::Conflict));
    let mut uids = f.uids();
    uids.sort();
    assert_eq!(uids, ["mail-1", "meet-1"]);
}

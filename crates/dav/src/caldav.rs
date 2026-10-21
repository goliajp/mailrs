//! CalDAV (RFC 4791) request handlers.
//!
//! Each public fn here takes a `&dyn CalendarStore` and a few request-derived
//! arguments (path segments, headers, body), runs the protocol logic, and
//! returns a [`DavResponse`] the server-side wrapper can hand to its
//! framework. [`crate::dispatch`] does the path and method routing for a
//! wrapper that wants it.
//!
//! Collection names are passed decoded; hrefs are emitted percent-encoded.

use std::fmt::Write;

use crate::error::DavError;
use crate::parse::{encode_segment, multiget_uids};
use crate::store::{CalendarStore, into_dav_error};
use crate::types::{Event, PutResult};
use crate::xml::{DavResponse, ctag_of, etag_of, multistatus, xml_escape};

fn calendar_href(user: &str, calendar: &str) -> String {
    format!("/dav/calendars/{user}/{}/", encode_segment(calendar))
}

fn event_href(user: &str, calendar: &str, uid: &str) -> String {
    format!(
        "{}{}.ics",
        calendar_href(user, calendar),
        encode_segment(uid)
    )
}

fn events_ctag(events: &[Event]) -> String {
    ctag_of(events.iter().map(|e| e.etag.as_str()))
}

/// PROPFIND on `/dav/calendars/{user}/` — the calendar home collection.
///
/// At `Depth: 0` returns only the collection itself; at `Depth: 1` (or
/// `infinity`) includes one `<D:response>` per child calendar, each with a
/// `getctag` that changes whenever any event in it does.
pub async fn calendar_home_propfind(
    store: &dyn CalendarStore,
    user: &str,
    depth: u32,
) -> Result<DavResponse, DavError> {
    store
        .ensure_default_calendar(user)
        .await
        .map_err(into_dav_error)?;

    let mut responses = format!(
        "<D:response>\n\
         <D:href>/dav/calendars/{user}/</D:href>\n\
         <D:propstat>\n<D:prop>\n\
         <D:resourcetype><D:collection/></D:resourcetype>\n\
         <D:displayname>Calendars</D:displayname>\n\
         </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
         </D:response>\n"
    );

    if depth >= 1 {
        let calendars = store.list_calendars(user).await.map_err(into_dav_error)?;
        for cal in &calendars {
            let events = store.list_events(cal.id).await.map_err(into_dav_error)?;
            let _ = write!(
                responses,
                "<D:response>\n\
                 <D:href>{}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:resourcetype><D:collection/><C:calendar/></D:resourcetype>\n\
                 <D:displayname>{}</D:displayname>\n\
                 <C:calendar-description>{}</C:calendar-description>\n\
                 <C:supported-calendar-component-set>\
                 <C:comp name=\"VEVENT\"/>\
                 </C:supported-calendar-component-set>\n\
                 <D:current-user-privilege-set>\
                 <D:privilege><D:all/></D:privilege>\
                 </D:current-user-privilege-set>\n\
                 <CS:getctag>{}</CS:getctag>\n\
                 <apple:calendar-color xmlns:apple=\"http://apple.com/ns/ical/\">{}</apple:calendar-color>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&calendar_href(user, &cal.name)),
                xml_escape(&cal.name),
                xml_escape(&cal.description),
                events_ctag(&events),
                xml_escape(&cal.color),
            );
        }
    }

    Ok(multistatus(&responses))
}

/// PROPFIND on `/dav/calendars/{user}/{calendar}/` — a single calendar.
///
/// At `Depth: 0` returns the calendar itself; at `Depth >= 1` lists every
/// event in the calendar (etag + content-type, no calendar-data body).
pub async fn calendar_propfind(
    store: &dyn CalendarStore,
    user: &str,
    calendar: &str,
    calendar_id: i64,
    depth: u32,
) -> Result<DavResponse, DavError> {
    let events = store
        .list_events(calendar_id)
        .await
        .map_err(into_dav_error)?;
    let mut responses = format!(
        "<D:response>\n\
         <D:href>{}</D:href>\n\
         <D:propstat>\n<D:prop>\n\
         <D:resourcetype><D:collection/><C:calendar/></D:resourcetype>\n\
         <D:displayname>{}</D:displayname>\n\
         <C:supported-calendar-component-set><C:comp name=\"VEVENT\"/></C:supported-calendar-component-set>\n\
         <CS:getctag>{}</CS:getctag>\n\
         </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
         </D:response>\n",
        xml_escape(&calendar_href(user, calendar)),
        xml_escape(calendar),
        events_ctag(&events),
    );

    if depth >= 1 {
        for ev in &events {
            let _ = write!(
                responses,
                "<D:response>\n\
                 <D:href>{}</D:href>\n\
                 <D:propstat>\n<D:prop>\n\
                 <D:getetag>\"{}\"</D:getetag>\n\
                 <D:getcontenttype>text/calendar; charset=utf-8</D:getcontenttype>\n\
                 </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
                 </D:response>\n",
                xml_escape(&event_href(user, calendar, &ev.uid)),
                ev.etag,
            );
        }
    }

    Ok(multistatus(&responses))
}

/// The `<D:response>` elements a calendar REPORT returns for one calendar.
///
/// `calendar-multiget` (RFC 4791 §7.9) keeps the requested UIDs;
/// `calendar-query` (§7.8) returns every event — its time-range filter is
/// intentionally not implemented, clients cope with the full set.
async fn report_responses(
    store: &dyn CalendarStore,
    user: &str,
    calendar: &str,
    calendar_id: i64,
    body: &str,
) -> Result<String, DavError> {
    let wanted = match body.contains("calendar-multiget") {
        true => Some(multiget_uids(body, ".ics")?),
        false => None,
    };
    let events = store
        .list_events(calendar_id)
        .await
        .map_err(into_dav_error)?;
    let mut responses = String::new();
    for ev in &events {
        if let Some(uids) = &wanted
            && !uids.contains(&ev.uid)
        {
            continue;
        }
        let _ = write!(
            responses,
            "<D:response>\n\
             <D:href>{}</D:href>\n\
             <D:propstat>\n<D:prop>\n\
             <D:getetag>\"{}\"</D:getetag>\n\
             <C:calendar-data>{}</C:calendar-data>\n\
             </D:prop>\n<D:status>HTTP/1.1 200 OK</D:status>\n</D:propstat>\n\
             </D:response>\n",
            xml_escape(&event_href(user, calendar, &ev.uid)),
            ev.etag,
            xml_escape(&ev.icalendar),
        );
    }
    Ok(responses)
}

/// REPORT on a calendar collection — `calendar-multiget` or `calendar-query`.
pub async fn calendar_report(
    store: &dyn CalendarStore,
    user: &str,
    calendar: &str,
    calendar_id: i64,
    body: &str,
) -> Result<DavResponse, DavError> {
    let responses = report_responses(store, user, calendar, calendar_id, body).await?;
    Ok(multistatus(&responses))
}

/// REPORT on the calendar home: the same report, run over every calendar.
///
/// Not in RFC 4791, which scopes reports to a collection, but some clients
/// send their query to the home and expect every event back.
pub async fn calendar_home_report(
    store: &dyn CalendarStore,
    user: &str,
    body: &str,
) -> Result<DavResponse, DavError> {
    let mut responses = String::new();
    for cal in store.list_calendars(user).await.map_err(into_dav_error)? {
        responses.push_str(&report_responses(store, user, &cal.name, cal.id, body).await?);
    }
    Ok(multistatus(&responses))
}

/// GET on `/dav/calendars/{user}/{calendar}/{uid}.ics`.
pub async fn event_get(
    store: &dyn CalendarStore,
    calendar_id: i64,
    uid: &str,
) -> Result<DavResponse, DavError> {
    let event = store
        .get_event(calendar_id, uid)
        .await
        .map_err(into_dav_error)?;
    match event {
        Some(ev) => Ok(DavResponse::new(200)
            .with_header("content-type", "text/calendar; charset=utf-8")
            .with_header("etag", &format!("\"{}\"", ev.etag))
            .with_body(ev.icalendar.into_bytes())),
        None => Err(DavError::NotFound),
    }
}

/// PUT on an event resource. Honours `If-Match` (current etag must match)
/// and `If-None-Match: *` (resource must not already exist).
///
/// 201 on create, 204 on update, `ETag` header either way. The body should
/// be valid RFC 5545 iCalendar; the store is responsible for parsing /
/// structured-column projection, and rejects what it cannot store.
pub async fn event_put(
    store: &dyn CalendarStore,
    calendar_id: i64,
    uid: &str,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
    body: &str,
) -> Result<DavResponse, DavError> {
    if if_match.is_some() || if_none_match.is_some() {
        let current = store
            .event_etag(calendar_id, uid)
            .await
            .map_err(into_dav_error)?;
        crate::precondition::check(current.as_deref(), if_match, if_none_match)?;
    }

    let etag = etag_of(body);
    let PutResult {
        created,
        etag: stored_etag,
    } = store
        .put_event(calendar_id, uid, body, &etag)
        .await
        .map_err(into_dav_error)?;

    let status = if created { 201 } else { 204 };
    Ok(DavResponse::new(status).with_header("etag", &format!("\"{stored_etag}\"")))
}

/// DELETE on an event resource. 204 on success, 404 when the event didn't
/// exist (RFC 4791 §5.3.2 mapping).
pub async fn event_delete(
    store: &dyn CalendarStore,
    calendar_id: i64,
    uid: &str,
) -> Result<DavResponse, DavError> {
    let deleted = store
        .delete_event(calendar_id, uid)
        .await
        .map_err(into_dav_error)?;
    if deleted {
        Ok(DavResponse::new(204))
    } else {
        Err(DavError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Calendar, Event};
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemStore {
        calendars: Mutex<Vec<(String, Calendar)>>, // (owner, cal)
        events: Mutex<Vec<(i64, Event)>>,          // (cal_id, event)
        next_cal_id: Mutex<i64>,
    }

    impl MemStore {
        fn add_calendar(&self, owner: &str, name: &str) -> i64 {
            let mut next = self.next_cal_id.lock().unwrap();
            *next += 1;
            let id = *next;
            let cal = Calendar {
                id,
                name: name.into(),
                color: "#abcdef".into(),
                description: "test".into(),
            };
            self.calendars.lock().unwrap().push((owner.into(), cal));
            id
        }
        fn add_event(&self, cal_id: i64, uid: &str, icalendar: &str) {
            self.events.lock().unwrap().push((
                cal_id,
                Event {
                    uid: uid.into(),
                    etag: etag_of(icalendar),
                    icalendar: icalendar.into(),
                    summary: "".into(),
                    dtstart: None,
                    dtend: None,
                },
            ));
        }
    }

    #[async_trait]
    impl CalendarStore for MemStore {
        async fn list_calendars(
            &self,
            user: &str,
        ) -> Result<Vec<Calendar>, crate::store::StoreError> {
            Ok(self
                .calendars
                .lock()
                .unwrap()
                .iter()
                .filter(|(o, _)| o == user)
                .map(|(_, c)| c.clone())
                .collect())
        }
        async fn get_calendar(
            &self,
            user: &str,
            name: &str,
        ) -> Result<Option<Calendar>, crate::store::StoreError> {
            Ok(self
                .calendars
                .lock()
                .unwrap()
                .iter()
                .find(|(o, c)| o == user && c.name == name)
                .map(|(_, c)| c.clone()))
        }
        async fn list_events(
            &self,
            calendar_id: i64,
        ) -> Result<Vec<Event>, crate::store::StoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|(c, _)| *c == calendar_id)
                .map(|(_, e)| e.clone())
                .collect())
        }
        async fn get_event(
            &self,
            calendar_id: i64,
            uid: &str,
        ) -> Result<Option<Event>, crate::store::StoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .find(|(c, e)| *c == calendar_id && e.uid == uid)
                .map(|(_, e)| e.clone()))
        }
        async fn event_etag(
            &self,
            calendar_id: i64,
            uid: &str,
        ) -> Result<Option<String>, crate::store::StoreError> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .find(|(c, e)| *c == calendar_id && e.uid == uid)
                .map(|(_, e)| e.etag.clone()))
        }
        async fn put_event(
            &self,
            calendar_id: i64,
            uid: &str,
            icalendar: &str,
            etag: &str,
        ) -> Result<PutResult, crate::store::StoreError> {
            let mut events = self.events.lock().unwrap();
            let pos = events
                .iter()
                .position(|(c, e)| *c == calendar_id && e.uid == uid);
            let created = pos.is_none();
            if let Some(p) = pos {
                events[p].1.icalendar = icalendar.into();
                events[p].1.etag = etag.into();
            } else {
                events.push((
                    calendar_id,
                    Event {
                        uid: uid.into(),
                        etag: etag.into(),
                        icalendar: icalendar.into(),
                        summary: "".into(),
                        dtstart: None,
                        dtend: None,
                    },
                ));
            }
            Ok(PutResult {
                created,
                etag: etag.into(),
            })
        }
        async fn delete_event(
            &self,
            calendar_id: i64,
            uid: &str,
        ) -> Result<bool, crate::store::StoreError> {
            let mut events = self.events.lock().unwrap();
            let before = events.len();
            events.retain(|(c, e)| !(*c == calendar_id && e.uid == uid));
            Ok(events.len() < before)
        }
        async fn ensure_default_calendar(
            &self,
            user: &str,
        ) -> Result<(), crate::store::StoreError> {
            let has_any = self
                .calendars
                .lock()
                .unwrap()
                .iter()
                .any(|(o, _)| o == user);
            if !has_any {
                let _ = self.add_calendar(user, "Default");
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn calendar_home_depth_zero_returns_only_collection() {
        let store = MemStore::default();
        store.add_calendar("u", "Work");
        let resp = calendar_home_propfind(&store, "u", 0).await.unwrap();
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("/dav/calendars/u/"));
        // child calendars only at Depth: 1+
        assert!(!body.contains("/dav/calendars/u/Work/"));
    }

    #[tokio::test]
    async fn calendar_home_depth_one_lists_child_calendars() {
        let store = MemStore::default();
        store.add_calendar("u", "Work");
        let resp = calendar_home_propfind(&store, "u", 1).await.unwrap();
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("/dav/calendars/u/Work/"));
    }

    #[tokio::test]
    async fn event_get_returns_icalendar() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        store.add_event(cid, "abc", "BEGIN:VCALENDAR\nEND:VCALENDAR");
        let resp = event_get(&store, cid, "abc").await.unwrap();
        assert_eq!(resp.status, 200);
        assert!(String::from_utf8(resp.body).unwrap().contains("VCALENDAR"));
    }

    #[tokio::test]
    async fn event_get_missing_returns_not_found_error() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        assert!(matches!(
            event_get(&store, cid, "missing").await,
            Err(DavError::NotFound)
        ));
    }

    #[tokio::test]
    async fn event_put_creates_then_updates() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        let resp = event_put(&store, cid, "abc", None, None, "BEGIN:VEVENT\nEND:VEVENT")
            .await
            .unwrap();
        assert_eq!(resp.status, 201);

        let resp2 = event_put(
            &store,
            cid,
            "abc",
            None,
            None,
            "BEGIN:VEVENT\nDESCRIPTION:updated\nEND:VEVENT",
        )
        .await
        .unwrap();
        assert_eq!(resp2.status, 204);
    }

    #[tokio::test]
    async fn event_put_if_none_match_star_blocks_overwrite() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        event_put(&store, cid, "abc", None, None, "BEGIN:VEVENT\nEND:VEVENT")
            .await
            .unwrap();
        let err = event_put(
            &store,
            cid,
            "abc",
            None,
            Some("*"),
            "BEGIN:VEVENT\nEND:VEVENT",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DavError::PreconditionFailed));
    }

    #[tokio::test]
    async fn event_put_if_match_blocks_stale_etag() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        event_put(&store, cid, "abc", None, None, "v1")
            .await
            .unwrap();
        let err = event_put(&store, cid, "abc", Some("\"deadbeefdeadbeef\""), None, "v2")
            .await
            .unwrap_err();
        assert!(matches!(err, DavError::PreconditionFailed));
    }

    #[tokio::test]
    async fn event_delete_returns_no_content_then_not_found() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        store.add_event(cid, "abc", "x");
        let r = event_delete(&store, cid, "abc").await.unwrap();
        assert_eq!(r.status, 204);
        let err = event_delete(&store, cid, "abc").await.unwrap_err();
        assert!(matches!(err, DavError::NotFound));
    }

    #[tokio::test]
    async fn calendar_report_multiget_filters_by_uid() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        store.add_event(cid, "a", "BEGIN:VEVENT\nUID:a\nEND:VEVENT");
        store.add_event(cid, "b", "BEGIN:VEVENT\nUID:b\nEND:VEVENT");
        let body = "<C:calendar-multiget xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\
                    <D:href>/dav/calendars/u/Work/a.ics</D:href></C:calendar-multiget>";
        let resp = calendar_report(&store, "u", "Work", cid, body)
            .await
            .unwrap();
        let text = String::from_utf8(resp.body).unwrap();
        assert!(text.contains("/dav/calendars/u/Work/a.ics"));
        assert!(!text.contains("/dav/calendars/u/Work/b.ics"));
    }

    #[tokio::test]
    async fn calendar_report_query_returns_all_events() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        store.add_event(cid, "a", "BEGIN:VEVENT\nUID:a\nEND:VEVENT");
        store.add_event(cid, "b", "BEGIN:VEVENT\nUID:b\nEND:VEVENT");
        let body = "<C:calendar-query xmlns:C=\"urn:ietf:params:xml:ns:caldav\"/>";
        let resp = calendar_report(&store, "u", "Work", cid, body)
            .await
            .unwrap();
        let text = String::from_utf8(resp.body).unwrap();
        assert!(text.contains("a.ics"));
        assert!(text.contains("b.ics"));
    }

    #[tokio::test]
    async fn event_delete_missing_returns_not_found() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        let r = event_delete(&store, cid, "ghost").await;
        assert!(matches!(r, Err(DavError::NotFound)));
    }

    #[tokio::test]
    async fn event_put_create_returns_201() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        let resp = event_put(&store, cid, "new", None, None, "ICS")
            .await
            .unwrap();
        assert_eq!(resp.status, 201);
    }

    #[tokio::test]
    async fn event_put_update_returns_204() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        let _ = event_put(&store, cid, "x", None, None, "v1").await.unwrap();
        let resp = event_put(&store, cid, "x", None, None, "v2").await.unwrap();
        assert_eq!(resp.status, 204);
    }

    #[tokio::test]
    async fn event_put_if_match_correct_etag_succeeds() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        let _ = event_put(&store, cid, "x", None, None, "v1").await.unwrap();
        let etag = store.event_etag(cid, "x").await.unwrap().unwrap();
        let if_match = format!("\"{etag}\"");
        let resp = event_put(&store, cid, "x", Some(&if_match), None, "v2")
            .await
            .unwrap();
        assert_eq!(resp.status, 204);
    }

    #[tokio::test]
    async fn event_put_if_match_wrong_etag_412() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        let _ = event_put(&store, cid, "x", None, None, "v1").await.unwrap();
        let r = event_put(&store, cid, "x", Some("\"deadbeef\""), None, "v2").await;
        assert!(matches!(r, Err(DavError::PreconditionFailed)));
    }

    #[tokio::test]
    async fn event_put_if_none_match_star_new_succeeds() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        let resp = event_put(&store, cid, "fresh", None, Some("*"), "ICS")
            .await
            .unwrap();
        assert_eq!(resp.status, 201);
    }

    #[tokio::test]
    async fn calendar_home_propfind_creates_default() {
        // Empty store + propfind on home → ensure_default_calendar fires
        let store = MemStore::default();
        let resp = calendar_home_propfind(&store, "newuser", 1).await.unwrap();
        // After call, default calendar should exist
        let cals = store.list_calendars("newuser").await.unwrap();
        assert_eq!(cals.len(), 1);
        assert_eq!(resp.status, 207);
    }

    #[tokio::test]
    async fn calendar_home_propfind_idempotent_default_creation() {
        let store = MemStore::default();
        let _ = calendar_home_propfind(&store, "u", 1).await.unwrap();
        let _ = calendar_home_propfind(&store, "u", 1).await.unwrap();
        let cals = store.list_calendars("u").await.unwrap();
        assert_eq!(cals.len(), 1, "default calendar must only be created once");
    }

    #[tokio::test]
    async fn calendar_propfind_depth_0_just_collection() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        store.add_event(cid, "a", "BEGIN:VEVENT\nUID:a\nEND:VEVENT");
        let resp = calendar_propfind(&store, "u", "Work", cid, 0)
            .await
            .unwrap();
        let text = String::from_utf8(resp.body).unwrap();
        assert!(text.contains("/dav/calendars/u/Work/"));
        assert!(!text.contains("a.ics"));
    }

    #[tokio::test]
    async fn calendar_propfind_depth_1_lists_events() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        store.add_event(cid, "a", "BEGIN:VEVENT\nUID:a\nEND:VEVENT");
        store.add_event(cid, "b", "BEGIN:VEVENT\nUID:b\nEND:VEVENT");
        let resp = calendar_propfind(&store, "u", "Work", cid, 1)
            .await
            .unwrap();
        let text = String::from_utf8(resp.body).unwrap();
        assert!(text.contains("a.ics"));
        assert!(text.contains("b.ics"));
    }

    #[tokio::test]
    async fn calendar_report_multiget_empty_uids() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        store.add_event(cid, "a", "BEGIN:VEVENT\nUID:a\nEND:VEVENT");
        // multiget marker but zero <D:href>
        let body = "<C:calendar-multiget xmlns:C=\"urn:ietf:params:xml:ns:caldav\"/>";
        let resp = calendar_report(&store, "u", "Work", cid, body)
            .await
            .unwrap();
        let text = String::from_utf8(resp.body).unwrap();
        assert!(!text.contains("a.ics"));
    }

    #[tokio::test]
    async fn calendar_report_multiget_missing_uid_filtered_out() {
        let store = MemStore::default();
        let cid = store.add_calendar("u", "Work");
        store.add_event(cid, "a", "BEGIN:VEVENT\nUID:a\nEND:VEVENT");
        let body = "<C:calendar-multiget xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\
            <D:href>/dav/calendars/u/Work/ghost.ics</D:href></C:calendar-multiget>";
        let resp = calendar_report(&store, "u", "Work", cid, body)
            .await
            .unwrap();
        let text = String::from_utf8(resp.body).unwrap();
        assert!(!text.contains("ghost.ics"));
        assert!(!text.contains("a.ics"));
    }

    #[tokio::test]
    async fn calendar_home_depth_zero_does_not_list_calendars() {
        let store = MemStore::default();
        store.add_calendar("u", "Work");
        store.add_calendar("u", "Personal");
        let resp = calendar_home_propfind(&store, "u", 0).await.unwrap();
        let text = String::from_utf8(resp.body).unwrap();
        // The home itself appears, but children must not
        assert!(text.contains("/dav/calendars/u/"));
        assert!(!text.contains("/Work/"));
        assert!(!text.contains("/Personal/"));
    }
}

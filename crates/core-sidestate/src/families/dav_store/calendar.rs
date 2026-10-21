//! Calendars: the user's own events, and one read-only calendar per feed.
//!
//! Every event lives in one per-user set of rows; which calendar it shows
//! in follows from its `source`. `feed:{id}` rows belong to that feed's
//! calendar, which the sync owns and CalDAV may only read. Everything
//! else — events made over CalDAV, invitations filed from mail — is in
//! [`DEFAULT_CALENDAR`].
//!
//! A row is keyed by the `UID` in its text, not by the resource name the
//! client PUT it to, so the client's copy of a meeting and the invitation
//! filed from mail are the same row. A name that differs from the UID is
//! kept in [`names`], and every href is built from the name.
//!
//! [`names`]: crate::families::calendar_events::names

use async_trait::async_trait;
use mailrs_dav::caldav::event_href;
use mailrs_dav::types::{Calendar, Event, PutResult};
use mailrs_dav::{CalendarStore, DavError, StoreError, etag_of};

use super::{DavStore, KevyAccess, bad_request, forbidden};
use crate::families::calendar_events::{self as rows, StoredEvent, names};
use crate::families::calendar_feeds::{FeedRow, feeds_key};

/// The id of the calendar holding the user's own events.
pub const DEFAULT_CALENDAR: i64 = 0;

const DEFAULT_NAME: &str = "default";

/// The id of a subscribed feed's calendar. Never [`DEFAULT_CALENDAR`].
pub fn feed_calendar_id(feed_id: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in feed_id.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    ((h >> 1) as i64).max(1)
}

fn calendar_of(ev: &StoredEvent) -> i64 {
    match ev.source.strip_prefix("feed:") {
        Some(feed) => feed_calendar_id(feed),
        None => DEFAULT_CALENDAR,
    }
}

/// `ev` as served at the resource `name`.
fn to_event(ev: &StoredEvent, name: &str) -> Event {
    let text = rows::to_ics(ev);
    let instant = |s: &Option<String>| {
        s.as_deref()
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
            .map(|d| d.with_timezone(&chrono::Utc))
    };
    Event {
        uid: name.to_string(),
        etag: etag_of(&text),
        summary: ev.summary.clone(),
        dtstart: instant(&ev.dtstart),
        dtend: instant(&ev.dtend),
        icalendar: text,
    }
}

/// Feed calendars are named after the feed; a name that would be
/// ambiguous gets the feed id appended, since the name is also the path.
fn feed_calendars(feeds: &[FeedRow]) -> Vec<Calendar> {
    feeds
        .iter()
        .map(|f| {
            let clash = f.name.is_empty()
                || f.name == DEFAULT_NAME
                || feeds.iter().filter(|o| o.name == f.name).count() > 1;
            let name = match clash {
                true => format!("{} ({})", f.name, f.id),
                false => f.name.clone(),
            };
            Calendar {
                id: feed_calendar_id(&f.id),
                name,
                color: f.color.clone().unwrap_or_default(),
                description: "Subscribed calendar (read-only)".into(),
            }
        })
        .collect()
}

fn read_feeds(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Vec<FeedRow>> {
    let flat = conn.hgetall(feeds_key(user).as_bytes())?;
    let mut feeds = Vec::new();
    for pair in flat.chunks(2) {
        if let [_, json] = pair {
            feeds.push(serde_json::from_slice::<FeedRow>(json).map_err(std::io::Error::other)?);
        }
    }
    feeds.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(feeds)
}

/// The row a request for the resource `name` reaches, in any calendar.
fn served_at(
    conn: &mut kevy_client::Connection,
    user: &str,
    name: &str,
) -> std::io::Result<Option<StoredEvent>> {
    match names::resolve(conn, user, name)? {
        Some(uid) => rows::get(conn, user, &uid),
        None => Ok(None),
    }
}

impl<A: KevyAccess> DavStore<A> {
    fn row_in(&self, calendar_id: i64, name: &str) -> Result<Option<StoredEvent>, StoreError> {
        let row = self.run(|c, user| served_at(c, user, name))?;
        Ok(row.filter(|ev| calendar_of(ev) == calendar_id))
    }

    /// Whether `row`, PUT to `name`, creates a resource; `Err` says why it
    /// cannot be stored.
    fn check_put(&self, name: &str, row: &StoredEvent) -> Result<bool, StoreError> {
        let current = self.run(|c, user| served_at(c, user, name))?;
        // a resource keeps its UID: a different one is a different event,
        // which belongs under a name of its own
        if current.as_ref().is_some_and(|ev| ev.uid != row.uid) {
            return Err(Box::new(DavError::Conflict));
        }
        let (existing, existing_name) = self.run(|c, user| {
            Ok((
                rows::get(c, user, &row.uid)?,
                names::name_of(c, user, &row.uid)?,
            ))
        })?;
        let Some(existing) = existing else {
            return Ok(true);
        };
        // the uid is taken by a subscribed calendar's event, which the sync
        // would overwrite; a client's copy belongs under a uid of its own
        if calendar_of(&existing) != DEFAULT_CALENDAR {
            return Err(forbidden());
        }
        match existing_name == name {
            true => Ok(current.is_none()),
            false => Err(Box::new(DavError::UidConflict(event_href(
                &self.user,
                DEFAULT_NAME,
                &existing_name,
            )))),
        }
    }
}

#[async_trait]
impl<A: KevyAccess> CalendarStore for DavStore<A> {
    async fn list_calendars(&self, user: &str) -> Result<Vec<Calendar>, StoreError> {
        self.owner(user)?;
        let feeds = self.run(read_feeds)?;
        let mut out = vec![Calendar {
            id: DEFAULT_CALENDAR,
            name: DEFAULT_NAME.into(),
            color: String::new(),
            description: "Events".into(),
        }];
        out.extend(feed_calendars(&feeds));
        Ok(out)
    }

    async fn get_calendar(&self, user: &str, name: &str) -> Result<Option<Calendar>, StoreError> {
        let all = self.list_calendars(user).await?;
        Ok(all.into_iter().find(|c| c.name == name))
    }

    async fn list_events(&self, calendar_id: i64) -> Result<Vec<Event>, StoreError> {
        let (all, hrefs) =
            self.run(|c, user| Ok((rows::list(c, user)?, names::hrefs(c, user)?)))?;
        Ok(all
            .iter()
            .filter(|ev| calendar_of(ev) == calendar_id)
            .map(|ev| to_event(ev, hrefs.get(&ev.uid).unwrap_or(&ev.uid)))
            .collect())
    }

    async fn get_event(&self, calendar_id: i64, name: &str) -> Result<Option<Event>, StoreError> {
        let row = self.row_in(calendar_id, name)?;
        Ok(row.map(|ev| to_event(&ev, name)))
    }

    async fn event_etag(&self, calendar_id: i64, name: &str) -> Result<Option<String>, StoreError> {
        Ok(self.get_event(calendar_id, name).await?.map(|e| e.etag))
    }

    async fn put_event(
        &self,
        calendar_id: i64,
        name: &str,
        icalendar: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        if calendar_id != DEFAULT_CALENDAR {
            return Err(forbidden());
        }
        let row = rows::from_ics(icalendar, "caldav").map_err(bad_request)?;
        let created = self.check_put(name, &row)?;
        self.run(|c, user| {
            rows::put(c, user, &row)?;
            names::link(c, user, name, &row.uid)
        })?;
        Ok(PutResult {
            created,
            etag: etag.to_string(),
        })
    }

    async fn delete_event(&self, calendar_id: i64, name: &str) -> Result<bool, StoreError> {
        if calendar_id != DEFAULT_CALENDAR {
            return Err(forbidden());
        }
        match self.row_in(calendar_id, name)? {
            Some(ev) => self.run(|c, user| rows::delete(c, user, &ev.uid)),
            None => Ok(false),
        }
    }

    async fn ensure_default_calendar(&self, user: &str) -> Result<(), StoreError> {
        // the default calendar is implicit: it is wherever the user's own
        // events are, so there is nothing to create
        self.owner(user)
    }
}

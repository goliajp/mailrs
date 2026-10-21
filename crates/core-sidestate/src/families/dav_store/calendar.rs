//! Calendars: the user's own events, and one read-only calendar per feed.
//!
//! Every event lives in one per-user set of rows; which calendar it shows
//! in follows from its `source`. `feed:{id}` rows belong to that feed's
//! calendar, which the sync owns and CalDAV may only read. Everything
//! else — events made over CalDAV, invitations filed from mail — is in
//! [`DEFAULT_CALENDAR`].

use async_trait::async_trait;
use mailrs_dav::types::{Calendar, Event, PutResult};
use mailrs_dav::{CalendarStore, StoreError, etag_of};

use super::{DavStore, KevyAccess, bad_request, forbidden};
use crate::families::calendar_events::{self as rows, StoredEvent};
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

fn to_event(ev: &StoredEvent) -> Event {
    let text = rows::to_ics(ev);
    let instant = |s: &Option<String>| {
        s.as_deref()
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
            .map(|d| d.with_timezone(&chrono::Utc))
    };
    Event {
        uid: ev.uid.clone(),
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

impl<A: KevyAccess> DavStore<A> {
    fn row_in(&self, calendar_id: i64, uid: &str) -> Result<Option<StoredEvent>, StoreError> {
        let row = self.run(|c, user| rows::get(c, user, uid))?;
        Ok(row.filter(|ev| calendar_of(ev) == calendar_id))
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
        let all = self.run(rows::list)?;
        Ok(all
            .iter()
            .filter(|ev| calendar_of(ev) == calendar_id)
            .map(to_event)
            .collect())
    }

    async fn get_event(&self, calendar_id: i64, uid: &str) -> Result<Option<Event>, StoreError> {
        Ok(self.row_in(calendar_id, uid)?.as_ref().map(to_event))
    }

    async fn event_etag(&self, calendar_id: i64, uid: &str) -> Result<Option<String>, StoreError> {
        Ok(self.get_event(calendar_id, uid).await?.map(|e| e.etag))
    }

    async fn put_event(
        &self,
        calendar_id: i64,
        uid: &str,
        icalendar: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        if calendar_id != DEFAULT_CALENDAR {
            return Err(forbidden());
        }
        let row = rows::from_ics(uid, icalendar, "caldav").map_err(bad_request)?;
        let existing = self.run(|c, user| rows::get(c, user, uid))?;
        // the uid is taken by a subscribed calendar's event, which the sync
        // would overwrite; a client's copy belongs under a uid of its own
        if existing
            .as_ref()
            .is_some_and(|ev| calendar_of(ev) != DEFAULT_CALENDAR)
        {
            return Err(forbidden());
        }
        self.run(|c, user| rows::put(c, user, &row))?;
        Ok(PutResult {
            created: existing.is_none(),
            etag: etag.to_string(),
        })
    }

    async fn delete_event(&self, calendar_id: i64, uid: &str) -> Result<bool, StoreError> {
        if calendar_id != DEFAULT_CALENDAR {
            return Err(forbidden());
        }
        if self.row_in(calendar_id, uid)?.is_none() {
            return Ok(false);
        }
        self.run(|c, user| rows::delete(c, user, uid))
    }

    async fn ensure_default_calendar(&self, user: &str) -> Result<(), StoreError> {
        // the default calendar is implicit: it is wherever the user's own
        // events are, so there is nothing to create
        self.owner(user)
    }
}

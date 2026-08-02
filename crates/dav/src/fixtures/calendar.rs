//! The in-memory `CalendarStore` used by the DAV tests.

use std::sync::RwLock;

use async_trait::async_trait;

use crate::store::{CalendarStore, StoreError};
use crate::types::{Calendar, Event, PutResult};

// =====================================================================
// Calendar store
// =====================================================================

/// In-memory [`CalendarStore`] backed by `Vec`s under an `RwLock`. Build
/// via [`Self::new`] + chainable `with_*` setters.
pub struct InMemoryCalendarStore {
    inner: RwLock<CalInner>,
}

struct CalInner {
    calendars: Vec<(String, Calendar)>, // (owner, Calendar)
    events: Vec<(i64, Event)>,          // (calendar_id, Event)
    default_created_for: Vec<String>,   // owners we auto-created a Default for

    list_calendars_error: Option<String>,
    get_calendar_error: Option<String>,
    list_events_error: Option<String>,
    get_event_error: Option<String>,
    event_etag_error: Option<String>,
    put_event_error: Option<String>,
    delete_event_error: Option<String>,
    ensure_default_error: Option<String>,
}

impl InMemoryCalendarStore {
    /// Construct an empty store.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(CalInner {
                calendars: Vec::new(),
                events: Vec::new(),
                default_created_for: Vec::new(),
                list_calendars_error: None,
                get_calendar_error: None,
                list_events_error: None,
                get_event_error: None,
                event_etag_error: None,
                put_event_error: None,
                delete_event_error: None,
                ensure_default_error: None,
            }),
        }
    }

    /// Append a calendar owned by `owner`. Use [`make_calendar`] for a sane default shape.
    pub fn with_calendar(self, owner: &str, cal: Calendar) -> Self {
        self.inner
            .write()
            .unwrap()
            .calendars
            .push((owner.to_string(), cal));
        self
    }

    /// Append an event to the given calendar id.
    pub fn with_event(self, calendar_id: i64, event: Event) -> Self {
        self.inner
            .write()
            .unwrap()
            .events
            .push((calendar_id, event));
        self
    }

    /// Make [`CalendarStore::list_calendars`] return an error carrying `msg`.
    pub fn list_calendars_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_calendars_error = Some(msg.to_string());
        self
    }

    /// Make [`CalendarStore::get_calendar`] return an error carrying `msg`.
    pub fn get_calendar_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().get_calendar_error = Some(msg.to_string());
        self
    }

    /// Make [`CalendarStore::list_events`] return an error carrying `msg`.
    pub fn list_events_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_events_error = Some(msg.to_string());
        self
    }

    /// Make [`CalendarStore::get_event`] return an error carrying `msg`.
    pub fn get_event_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().get_event_error = Some(msg.to_string());
        self
    }

    /// Make [`CalendarStore::event_etag`] return an error carrying `msg`.
    pub fn event_etag_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().event_etag_error = Some(msg.to_string());
        self
    }

    /// Make [`CalendarStore::put_event`] return an error carrying `msg`.
    pub fn put_event_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().put_event_error = Some(msg.to_string());
        self
    }

    /// Make [`CalendarStore::delete_event`] return an error carrying `msg`.
    pub fn delete_event_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().delete_event_error = Some(msg.to_string());
        self
    }

    /// Make the `ensure_default_*` trait method return an error carrying `msg`.
    pub fn ensure_default_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().ensure_default_error = Some(msg.to_string());
        self
    }

    /// Read back every event currently stored for `calendar_id`. Tests use this to assert handler-driven mutations.
    pub fn events_in(&self, calendar_id: i64) -> Vec<Event> {
        self.inner
            .read()
            .unwrap()
            .events
            .iter()
            .filter(|(c, _)| *c == calendar_id)
            .map(|(_, e)| e.clone())
            .collect()
    }

    /// `true` when [`CalendarStore::ensure_default_calendar`] fired for `user`.
    pub fn default_calendar_was_created_for(&self, user: &str) -> bool {
        self.inner
            .read()
            .unwrap()
            .default_created_for
            .iter()
            .any(|u| u == user)
    }
}

impl Default for InMemoryCalendarStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CalendarStore for InMemoryCalendarStore {
    async fn list_calendars(&self, user: &str) -> Result<Vec<Calendar>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_calendars_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .calendars
            .iter()
            .filter(|(o, _)| o == user)
            .map(|(_, c)| c.clone())
            .collect())
    }

    async fn get_calendar(
        &self,
        user: &str,
        calendar_name: &str,
    ) -> Result<Option<Calendar>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.get_calendar_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .calendars
            .iter()
            .find(|(o, c)| o == user && c.name == calendar_name)
            .map(|(_, c)| c.clone()))
    }

    async fn list_events(&self, calendar_id: i64) -> Result<Vec<Event>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_events_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .events
            .iter()
            .filter(|(c, _)| *c == calendar_id)
            .map(|(_, e)| e.clone())
            .collect())
    }

    async fn get_event(&self, calendar_id: i64, uid: &str) -> Result<Option<Event>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.get_event_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .events
            .iter()
            .find(|(c, e)| *c == calendar_id && e.uid == uid)
            .map(|(_, e)| e.clone()))
    }

    async fn event_etag(&self, calendar_id: i64, uid: &str) -> Result<Option<String>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.event_etag_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .events
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
    ) -> Result<PutResult, StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.put_event_error {
            return Err(msg.clone().into());
        }
        let pos = inner
            .events
            .iter()
            .position(|(c, e)| *c == calendar_id && e.uid == uid);
        let created = pos.is_none();
        if let Some(p) = pos {
            inner.events[p].1.icalendar = icalendar.to_string();
            inner.events[p].1.etag = etag.to_string();
        } else {
            inner.events.push((
                calendar_id,
                Event {
                    uid: uid.to_string(),
                    etag: etag.to_string(),
                    icalendar: icalendar.to_string(),
                    summary: String::new(),
                    dtstart: None,
                    dtend: None,
                },
            ));
        }
        Ok(PutResult {
            created,
            etag: etag.to_string(),
        })
    }

    async fn delete_event(&self, calendar_id: i64, uid: &str) -> Result<bool, StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.delete_event_error {
            return Err(msg.clone().into());
        }
        let before = inner.events.len();
        inner
            .events
            .retain(|(c, e)| !(*c == calendar_id && e.uid == uid));
        Ok(inner.events.len() < before)
    }

    async fn ensure_default_calendar(&self, user: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.ensure_default_error {
            return Err(msg.clone().into());
        }
        let has_any = inner.calendars.iter().any(|(o, _)| o == user);
        if !has_any {
            let next_id = (inner.calendars.len() as i64) + 1;
            inner.calendars.push((
                user.to_string(),
                Calendar {
                    id: next_id,
                    name: "Default".to_string(),
                    color: String::new(),
                    description: String::new(),
                },
            ));
            inner.default_created_for.push(user.to_string());
        }
        Ok(())
    }
}

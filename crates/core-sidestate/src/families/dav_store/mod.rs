//! CalDAV / CardDAV storage over the rows the rest of the product reads.
//!
//! [`DavStore`] implements `mailrs_dav`'s [`CalendarStore`] and
//! [`AddressBookStore`] over [`calendar_events`] and [`contact_cards`], so
//! an event a phone saves is the row the web's conflicts check reads, an
//! invitation filed from mail is an event the phone sees, and a card saved
//! over CardDAV is offered by recipient autocomplete.
//!
//! One store per request, bound to the authenticated user: calendar and
//! book ids are only meaningful within that user's data, and every method
//! that is handed a user refuses any other with 403.
//!
//! [`calendar_events`]: super::calendar_events
//! [`contact_cards`]: super::contact_cards
//! [`CalendarStore`]: mailrs_dav::CalendarStore
//! [`AddressBookStore`]: mailrs_dav::AddressBookStore

use std::io;
use std::sync::Mutex;

use kevy_client::Connection;
use mailrs_dav::{DavError, StoreError};

mod book;
mod calendar;

pub use calendar::{DEFAULT_CALENDAR, feed_calendar_id};

/// How a [`DavStore`] reaches the network kevy. Blocking: an async caller
/// arranges for that (webapi's pool moves the call off the reactor).
pub trait KevyAccess: Send + Sync {
    /// Run `f` on a connection.
    fn with_conn<T>(&self, f: impl FnOnce(&mut Connection) -> io::Result<T>) -> io::Result<T>;
}

/// One connection behind a lock — for tests and one-shot tools.
pub struct SingleConn(pub Mutex<Connection>);

impl KevyAccess for SingleConn {
    fn with_conn<T>(&self, f: impl FnOnce(&mut Connection) -> io::Result<T>) -> io::Result<T> {
        let mut conn = self
            .0
            .lock()
            .map_err(|_| io::Error::other("kevy connection lock poisoned"))?;
        f(&mut conn)
    }
}

/// The DAV store for one authenticated user.
pub struct DavStore<A: KevyAccess> {
    user: String,
    access: A,
}

impl<A: KevyAccess> DavStore<A> {
    /// A store serving `user`'s calendars and contacts only.
    pub fn new(user: impl Into<String>, access: A) -> Self {
        Self {
            user: user.into(),
            access,
        }
    }

    fn owner(&self, user: &str) -> Result<(), StoreError> {
        match user == self.user {
            true => Ok(()),
            false => Err(Box::new(DavError::Forbidden)),
        }
    }

    fn run<T>(
        &self,
        f: impl FnOnce(&mut Connection, &str) -> io::Result<T>,
    ) -> Result<T, StoreError> {
        let user = self.user.as_str();
        self.access
            .with_conn(|c| f(c, user))
            .map_err(|e| Box::new(e) as StoreError)
    }
}

fn bad_request(why: String) -> StoreError {
    Box::new(DavError::BadRequest(why))
}

fn forbidden() -> StoreError {
    Box::new(DavError::Forbidden)
}

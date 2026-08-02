//! In-memory [`CalendarStore`] and
//! [`AddressBookStore`] implementations
//! suitable for tests, examples, and downstream-consumer test harnesses.
//!
//! **Intended use is testing.** Both stores keep every value in process
//! memory and never persist across restarts; do not wire either into a real
//! deployment.
//!
//! ## Quick start
//!
//! ```
//! use mailrs_dav::fixtures::{InMemoryCalendarStore, EXAMPLE_USER, make_calendar};
//!
//! let store = InMemoryCalendarStore::new()
//!     .with_calendar(EXAMPLE_USER, make_calendar(1, "Work"));
//! ```
//!
//! ## What it gives you
//!
//! - Stateful in-memory storage with builder APIs — `with_calendar`,
//!   `with_event`, `with_book`, `with_contact`.
//! - Per-method error injection via `<method>_fails` setters so each error
//!   path in your handler-driving code can be isolated in a single test.
//! - Read-back helpers for assertions — `events_in(cal_id)`,
//!   `contacts_in(book_id)`.
//! - Convenience constructors — `make_calendar`, `make_event`, `make_book`,
//!   `make_contact`.
//! - Response-inspection helpers for testing handler output —
//!   [`body_as_str`], [`header_value`].
//!
//! Used internally by this crate's integration tests; the same module is
//! exposed to downstream consumers so they can drive their own handler tests
//! without re-implementing the stores.

/// Convenience example user used by the constructors in this module. The
/// store does not assume any particular value — callers can use their own.
pub const EXAMPLE_USER: &str = "alice@example.com";

mod addressbook;
mod builders;
mod calendar;

pub use addressbook::*;
pub use builders::*;
pub use calendar::*;

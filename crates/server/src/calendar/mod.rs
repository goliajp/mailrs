//! Calendar event persistence + queries.
//!
//! All `calendar_events` table access funnels through this module so that
//! MRS-3..MRS-9 can evolve the schema and query patterns in one place.
//! Built on top of the [`crate::ical`] parser/serializer (RFC 5545 / 5546)
//! to keep the iTIP semantic layer separate from SQL plumbing.

// Several re-exports are unused at MRS-3 land time; they get callers as
// MRS-5 / MRS-6 / MRS-7 wire up. Drop this allow when all callers exist.
#![allow(unused_imports)]

pub mod event;
pub mod invite_extract;
pub mod reconcile;

pub use event::{
    delete_by_uid, find_by_uid, find_conflicts, upsert_from_parsed_invite, CalendarEventRow,
};

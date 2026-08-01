//! One module per side-state family; every handler is generic over
//! `S: crate::NetKevy` so both cores mount the same code.

pub mod admin_state;
pub mod analysis;
pub mod calendar_feeds;
pub mod contacts;
pub mod groups_admin;
pub mod identity_link;
pub mod outbound;
pub mod prefs;
pub mod send;
pub mod send_read;
pub mod suppression;
pub mod webhook_outbox;
pub mod webhooks;

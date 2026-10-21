//! One module per side-state family; every handler is generic over
//! `S: crate::NetKevy` so both cores mount the same code.

pub mod account_names;
pub mod admin_state;
pub mod analysis;
pub mod calendar_events;
pub mod calendar_feeds;
pub mod contacts;
pub mod correspondent;
pub mod dkim_signers;
pub mod domain_history;
pub mod email_groups;
pub mod groups_admin;
pub mod identity_link;
pub mod mock;
pub mod outbound;
pub mod permission_groups;
pub mod prefs;
pub mod reply_rotation;
pub mod send;
pub mod send_read;
pub mod sender_lists;
pub mod suppression;
pub mod webhook_outbox;
pub mod webhooks;

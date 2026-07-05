//! Per-method handler bodies for the pg-core's mailrs-core-api server.
//!
//! Only the SWITCHABLE mail-store families live here (conversations /
//! mailbox / message / thread / admin accounts+aliases+domains). The
//! shared network-kevy side-state families (drafts/signatures/templates/
//! reactions/webhooks/audit/contacts/analysis/outbound/groups/api-keys/
//! sieve) are mounted from `mailrs-core-sidestate` so both cores serve
//! them from the same network kevy — see `build_full_router`.

pub mod admin;
pub mod conversation;
pub mod mailbox;
pub mod message;
pub mod thread;

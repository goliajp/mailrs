//! Fastcore-specific core-api handlers that back onto fastcore's OWN
//! stores (embedded kevy mail store + maildir IMAP backend) — the
//! switchable mail-store surface. The SHARED side-state families
//! (drafts/signatures/templates/reactions/webhooks/audit/contacts/
//! analysis/outbound/groups/apikeys/sieve) live in `mailrs-core-sidestate`
//! and are mounted generically by both cores.

pub mod mail_admin;
pub mod mailbox;
pub mod message;

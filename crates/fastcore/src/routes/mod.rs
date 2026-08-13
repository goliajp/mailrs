//! Fastcore-specific core-api handlers that back onto fastcore's OWN
//! stores (embedded kevy mail store + maildir IMAP backend) — the
//! switchable mail-store surface. The SHARED side-state families
//! (drafts/signatures/templates/reactions/webhooks/audit/contacts/
//! analysis/outbound/groups/apikeys/sieve) live in `mailrs-core-sidestate`
//! and are mounted generically by both cores.
//!
//! The handlers themselves moved out of `lib.rs` on 2026-08-02, grouped by
//! what they do to a conversation rather than by the order someone wrote
//! them: `account` (accounts and aliases), `thread_actions` (the verbs the
//! list issues), `reads` (lists, search, counts, contents).

mod account;
pub(crate) mod message_ops;
mod reads;
mod thread_actions;

pub(crate) use account::*;
pub(crate) use message_ops::*;
pub(crate) use reads::*;
pub(crate) use thread_actions::*;

pub mod mail_admin;
pub mod mailbox;
pub mod message;

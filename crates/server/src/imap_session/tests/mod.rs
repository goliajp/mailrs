//! Test module for `imap_session`.
//!
//! Each sibling test file exercises one slice of behavior
//! (matches the one-file-one-thing policy):
//!
//! - [`integration`] — async tests against a real PG via
//!   `MAILRS_PG_URL`; tagged `#[ignore]` so the default
//!   `cargo test` run skips them.
//! - [`matchers`] — `message_matches_criteria` SEARCH-key
//!   evaluation against synthetic `MessageMeta`.
//! - [`formatters`] — pure unit tests for the IMAP wire-format
//!   helpers re-exported from `mailrs-imap-format`.

mod formatters;
mod integration;
mod matchers;

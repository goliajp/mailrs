#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! IMAP protocol parser and response formatter.
//!
//! `mailrs-imap-proto` implements the wire-format pieces of [RFC 3501]
//! (IMAP4rev1) — tagged command parsing, sequence-set arithmetic, and
//! response line formatting. It does no I/O and owns no state; it's the
//! protocol layer underneath an IMAP server you write yourself.
//!
//! This crate underpins the IMAP server in [mailrs], a Rust mail server,
//! and is published independently so other Rust projects can reuse the
//! parsing layer.
//!
//! # Quick start
//!
//! ```
//! use mailrs_imap_proto::{parse_command, ImapCommand, format_capability};
//!
//! // parse a wire-format tagged command line
//! let parsed = parse_command("a001 CAPABILITY").unwrap();
//! assert_eq!(parsed.tag, "a001");
//! assert_eq!(parsed.command, ImapCommand::Capability);
//!
//! // format the matching response
//! let resp = format_capability(&["IMAP4rev1", "IDLE", "AUTH=PLAIN"]);
//! assert_eq!(resp, "* CAPABILITY IMAP4rev1 IDLE AUTH=PLAIN\r\n");
//! ```
//!
//! # What this crate does
//!
//! - **Parsing**: [`parse_command`] turns a tagged command line into a
//!   [`TaggedCommand`] containing a typed [`ImapCommand`] enum. Covers
//!   LOGIN / SELECT / FETCH / STORE / SEARCH / IDLE / APPEND / UID / etc.
//! - **Sequence sets**: [`parse_sequence_set`] parses IMAP sequence-set
//!   syntax (`"1"`, `"1:5"`, `"5:*"`, `"1,3,5:10"`) into a [`SequenceSet`]
//!   enum; [`sequence_set_to_uids`] expands it to a concrete UID vec.
//! - **Search**: [`parse_search_criteria`] converts an IMAP SEARCH key list
//!   to typed [`SearchKey`] values.
//! - **Response formatting**: [`format_ok`] / [`format_no`] / [`format_bad`]
//!   for tagged responses; [`format_capability`] / [`format_list`] /
//!   [`format_fetch`] / [`format_flags`] / [`format_exists`] /
//!   [`format_recent`] / [`format_bye`] / [`format_quota`] /
//!   [`format_quotaroot`] for untagged responses.
//!
//! # What this crate does NOT do
//!
//! - No I/O. No TCP, no TLS, no async runtime, no connection management.
//! - No mailbox storage or message indexing.
//! - No session state machine. Unlike `mailrs-smtp-proto`, IMAP's
//!   per-connection state (selected mailbox, capability negotiation,
//!   pending IDLE) is owned by the caller — this crate just gives typed
//!   commands and formatted replies.
//!
//! [RFC 3501]: https://datatracker.ietf.org/doc/html/rfc3501
//! [mailrs]: https://github.com/goliajp/mailrs

/// IMAP4rev1 command parser + `ImapCommand` AST.
pub mod command;
pub mod response;
/// Sequence-set parser + expansion (`1:10,12,*`).
pub mod sequence;

pub use command::{
    ImapCommand, ParseError, SearchKey, TaggedCommand, parse_command, parse_search_criteria,
};
pub use response::*;
pub use sequence::{SequenceSet, parse_sequence_set, sequence_set_to_uids};

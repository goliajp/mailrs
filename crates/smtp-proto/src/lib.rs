#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! SMTP protocol parser, formatter, and session state machine.
//!
//! `mailrs-smtp-proto` is a zero-I/O implementation of [RFC 5321] (SMTP) and
//! its common extensions: STARTTLS ([RFC 3207]), AUTH PLAIN / LOGIN
//! ([RFC 4954]), and SMTPUTF8 ([RFC 6531]). It parses incoming command lines
//! into typed `Command` enums, formats outgoing responses, and drives the
//! SMTP session state machine — but it never touches the network. The caller
//! owns all I/O.
//!
//! This crate underpins the inbound SMTP server in [mailrs], a Rust mail
//! server, and is published independently so other Rust projects can reuse
//! the parsing + state-machine layer without pulling in a full server.
//!
//! # Quick start
//!
//! ```
//! use mailrs_smtp_proto::{parse_command, Command, Session, SessionConfig, Event};
//!
//! // parse a wire-format command line
//! let cmd = parse_command("EHLO mail.example.com").unwrap();
//! assert!(matches!(cmd, Command::Ehlo("mail.example.com")));
//!
//! // drive the session state machine
//! let mut session = Session::new("smtp.example.com", SessionConfig::default());
//! let event = session.handle_command(&cmd);
//! assert!(matches!(event, Event::Reply(_)));
//! ```
//!
//! # What this crate does
//!
//! - **Parsing**: [`parse_command`] turns a wire-format command line into a
//!   borrowed [`Command<'_>`] enum (zero-copy).
//! - **Formatting**: [`Response`] and its constructors produce wire-format
//!   reply lines; [`format_ehlo_response`] handles multi-line EHLO.
//! - **State machine**: [`Session`] tracks the SMTP transaction state and
//!   maps `Command` → [`Event`] decisions (reply / open DATA / start TLS /
//!   authenticate / shut down).
//! - **DATA helpers**: [`unstuff_line`] / [`unstuff_data`] handle the
//!   dot-stuffing convention used in the DATA stage.
//! - **SASL**: [`auth::decode_plain`] / [`auth::decode_login_response`]
//!   decode AUTH PLAIN / LOGIN payloads.
//!
//! # What this crate does NOT do
//!
//! - No I/O. No TCP, no TLS, no async runtime. The caller wires it.
//! - No content scanning, no message storage, no DKIM/SPF/DMARC.
//! - No outbound SMTP client. See `mailrs-smtp-client` for that.
//!
//! [RFC 5321]: https://datatracker.ietf.org/doc/html/rfc5321
//! [RFC 3207]: https://datatracker.ietf.org/doc/html/rfc3207
//! [RFC 4954]: https://datatracker.ietf.org/doc/html/rfc4954
//! [RFC 6531]: https://datatracker.ietf.org/doc/html/rfc6531
//! [mailrs]: https://github.com/goliajp/mailrs

pub mod address;
pub mod auth;
pub mod command;
pub mod data;
pub mod parse;
pub mod response;
pub mod session;

pub use command::{AuthMechanism, Command, ForwardPath, Param, ReversePath};
pub use data::{unstuff_data, unstuff_line};
pub use parse::{ParseError, parse_command};
pub use response::{EnhancedCode, Response, format_ehlo_response};
pub use session::{
    AuthStep, Event, MAX_MESSAGE_SIZE, MAX_RECIPIENTS, Session, SessionConfig, State,
};

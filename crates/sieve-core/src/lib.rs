#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Native RFC 5228 Sieve interpreter.
//!
//! The internal engine `mailrs-sieve` (the wrapper) will route to
//! once it reaches differential parity with `sieve-rs` (v8 ckpt 6
//! swap). This crate is Apache-2.0 OR MIT — no AGPL, no
//! `deny.toml` exception.
//!
//! ## Quick start
//!
//! ```
//! use mailrs_sieve_core::{Action, eval_script};
//!
//! let script = r#"
//!     require ["fileinto"];
//!     if header :is "Subject" "spam" {
//!         fileinto "Junk";
//!     } else {
//!         keep;
//!     }
//! "#;
//! let message = b"Subject: spam\r\n\r\nbody\r\n";
//! let actions = eval_script(script, message).unwrap();
//! assert_eq!(actions, vec![Action::FileInto("Junk".into())]);
//! ```
//!
//! ## Status
//!
//! 0.1 first slice — RFC 5228 base only. See `CHANGELOG.md`.

mod ast;
mod eval;
mod lex;
mod parse;

pub use ast::{Action, Argument, Command, MatchType, Test};
pub use eval::{EvalError, eval_script};
pub use lex::{Token, TokenizeError, tokenize};
pub use parse::{ParseError, parse_script};

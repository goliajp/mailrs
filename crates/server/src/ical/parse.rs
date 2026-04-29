//! RFC 5545 §3.1 text → AST.
//!
//! Hand-rolled byte-by-byte tokenizer + state machine. Handles:
//! - line folding / unfolding (CRLF + leading whitespace continuation)
//! - property line `key[;param=val[,val]*]:value` with quoted parameter values
//! - text escapes (`\\`, `\,`, `\;`, `\n`, `\N`)
//! - component nesting via BEGIN / END pairing
//!
//! No parser combinator deps. Style aligned with `smtp-proto::parse`.
//!
//! MRS-2 phase 1: function signature only. Real implementation lands in the
//! next commits driven by RED → GREEN against fixtures under
//! `~/workspace/claws/MRS-1/fixtures/itip/`.

use super::{IcalError, RawComponent};

/// Parse a complete VCALENDAR document into a raw component tree.
///
/// The returned [`RawComponent`] is always named `VCALENDAR`; its children
/// include 0..N `VEVENT` plus optional `VTIMEZONE` blocks.
pub fn parse_calendar(_input: &str) -> Result<RawComponent, IcalError> {
    Err(IcalError::InvalidSyntax(
        "parse::parse_calendar not yet implemented".into(),
    ))
}

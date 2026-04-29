//! [`ParsedInvite`] → RFC 5545 text.
//!
//! Reverse of [`parse`] + [`semantics`]. Required for iTIP REPLY generation
//! in MRS-6 (where mailrs flips PARTSTAT and ships the modified invite back
//! to the ORGANIZER through the outbound queue).
//!
//! Output guarantees:
//! - strict line folding (75 octets per line, CRLF + space continuation)
//! - text escaping (`\\`, `\,`, `\;`, `\n`)
//! - UID and SEQUENCE preserved byte-for-byte from input
//! - DTSTAMP rewritten to "now" (REPLY semantics)
//!
//! MRS-2 phase 1: signature only.

use super::{IcalError, ParsedInvite};

/// Serialize a [`ParsedInvite`] to a complete VCALENDAR text suitable for
/// embedding as `text/calendar; method=<method>` in an iTIP MIME part.
pub fn serialize(_invite: &ParsedInvite) -> Result<String, IcalError> {
    Err(IcalError::InvalidSemantics(
        "serialize::serialize not yet implemented".into(),
    ))
}

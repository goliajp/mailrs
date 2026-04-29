//! Inline VTIMEZONE handling with chrono-tz fallback.
//!
//! Resolution order for a TZID reference:
//! 1. If TZID is a known IANA name (e.g. `America/New_York`) → resolve via
//!    `chrono_tz::Tz::from_str`.
//! 2. If TZID matches a known Outlook-style alias (e.g. `Tokyo Standard Time`,
//!    `Pacific Standard Time`) → map to the IANA equivalent and use chrono-tz.
//! 3. Otherwise → walk the inline VTIMEZONE block's STANDARD / DAYLIGHT
//!    subcomponents, build a custom offset function from their RRULEs and
//!    DTSTART / TZOFFSETFROM / TZOFFSETTO. This is the "real" self-built path.
//!
//! MRS-2 phase 1: signature only.

use super::VTimezone;
use chrono::NaiveDateTime;

/// Result of resolving a `TZID` reference against chrono-tz + inline VTIMEZONE blocks.
#[derive(Debug, Clone)]
pub enum ResolvedTz {
    /// IANA tz from chrono-tz.
    Iana(chrono_tz::Tz),
    /// Custom offset table built from an inline VTIMEZONE block.
    /// First-version representation: a list of (effective_from, utc_offset_seconds)
    /// transitions; richer representations land when MRS-9 needs them.
    Custom(Vec<(NaiveDateTime, i32)>),
}

/// Resolve a TZID against the available VTIMEZONE blocks plus chrono-tz fallback.
pub fn resolve(_tzid: &str, _inline_blocks: &[VTimezone]) -> Option<ResolvedTz> {
    None
}

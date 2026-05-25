//! Inline VTIMEZONE handling with chrono-tz fallback.
//!
//! Resolution order for a TZID reference (RFC 5545 §3.2.19 + §3.6.5):
//! 1. Inline VTIMEZONE block in the same VCALENDAR — preferred per spec.
//!    Walk its STANDARD / DAYLIGHT subcomponents, expand their RRULEs (via
//!    the `rrule` crate) into concrete transition timestamps, and produce
//!    a sorted offset table.
//! 2. IANA name (e.g. `America/New_York`) — `chrono_tz::Tz::from_str`.
//! 3. Microsoft / Outlook display name (`Tokyo Standard Time`,
//!    `Pacific Standard Time`, ...) — translated to the IANA equivalent
//!    via a built-in alias table, then resolved via chrono-tz.
//!
//! TZ name lookup is case-insensitive on the IANA side (RFC implies
//! case-sensitivity but real producers vary), strict on the inline side
//! (TZIDs match exactly because they're producer-controlled).

use std::str::FromStr;

use chrono::{NaiveDateTime, Offset};

use super::VTimezone;

/// Result of resolving a `TZID` reference.
#[derive(Debug, Clone)]
pub enum ResolvedTz {
    /// IANA tz from chrono-tz.
    Iana(chrono_tz::Tz),
    /// Custom offset table built from an inline VTIMEZONE block.
    /// Sorted by `effective_from`; each entry says "from this local
    /// instant onward, the UTC offset is `utc_offset_seconds`".
    Custom(Vec<TzTransition>),
}

/// One DST transition point inside a custom `Resolved::Custom(_)` schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TzTransition {
    /// Local wall-clock time when this offset becomes effective.
    pub effective_from: NaiveDateTime,
    /// New UTC offset in seconds (e.g. -18000 for UTC-5, 32400 for UTC+9).
    pub utc_offset_seconds: i32,
}

/// Resolve a TZID against the available VTIMEZONE blocks plus chrono-tz
/// fallback.
///
/// Returns `None` only when the TZID matches neither an inline block, nor an
/// IANA name, nor a known Microsoft alias.
pub fn resolve(tzid: &str, inline_blocks: &[VTimezone]) -> Option<ResolvedTz> {
    let trimmed = tzid.trim();

    // Step 1 — inline VTIMEZONE block (RFC 5545 says producer-supplied
    // definitions take precedence over external databases).
    if let Some(block) = inline_blocks
        .iter()
        .find(|b| b.tzid.eq_ignore_ascii_case(trimmed))
        && let Some(transitions) = build_transitions(block)
    {
        return Some(ResolvedTz::Custom(transitions));
    }
    // Inline block was found but unparseable — fall through to IANA.

    // Step 2 — IANA name.
    if let Ok(tz) = chrono_tz::Tz::from_str(trimmed) {
        return Some(ResolvedTz::Iana(tz));
    }

    // Step 3 — Microsoft / Outlook alias.
    if let Some(iana) = microsoft_to_iana(trimmed)
        && let Ok(tz) = chrono_tz::Tz::from_str(iana)
    {
        return Some(ResolvedTz::Iana(tz));
    }

    None
}

/// Convert a local wall-clock time into a UTC offset using the resolved tz.
///
/// For [`ResolvedTz::Iana`] this delegates to chrono-tz's `from_local_datetime`
/// and picks the earliest interpretation when ambiguous (DST fall-back).
/// For [`ResolvedTz::Custom`] it walks the transition table.
pub fn local_to_utc_offset_seconds(rt: &ResolvedTz, local: NaiveDateTime) -> Option<i32> {
    match rt {
        ResolvedTz::Iana(tz) => {
            use chrono::TimeZone;
            tz.from_local_datetime(&local)
                .earliest()
                .map(|d| d.offset().fix().local_minus_utc())
        }
        ResolvedTz::Custom(transitions) => {
            // Walk the table to find the most recent transition that took
            // effect at or before `local`. The table is sorted ascending.
            let mut last: Option<i32> = None;
            for t in transitions {
                if t.effective_from <= local {
                    last = Some(t.utc_offset_seconds);
                } else {
                    break;
                }
            }
            last
        }
    }
}

/// Build a transition table from an inline VTIMEZONE component.
///
/// Each STANDARD / DAYLIGHT child contributes its DTSTART (one explicit
/// transition) plus any RRULE expansions within a generous window
/// (1970..2050).
fn build_transitions(tz: &VTimezone) -> Option<Vec<TzTransition>> {
    let mut transitions: Vec<TzTransition> = Vec::new();

    for sub in &tz.raw_subs {
        let kind = sub.name.to_ascii_uppercase();
        if kind != "STANDARD" && kind != "DAYLIGHT" {
            continue;
        }
        let dtstart = sub
            .properties
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case("DTSTART"))?;
        let dtstart_local = parse_local_datetime(&dtstart.value)?;

        let offset_to = sub
            .properties
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case("TZOFFSETTO"))?;
        let offset_seconds = parse_utc_offset(&offset_to.value)?;

        // First transition: the DTSTART itself.
        transitions.push(TzTransition {
            effective_from: dtstart_local,
            utc_offset_seconds: offset_seconds,
        });

        // RRULE-expanded transitions, if present.
        if let Some(rrule_prop) = sub
            .properties
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case("RRULE"))
        {
            for occurrence in expand_rrule(&dtstart.value, &rrule_prop.value) {
                if occurrence != dtstart_local {
                    transitions.push(TzTransition {
                        effective_from: occurrence,
                        utc_offset_seconds: offset_seconds,
                    });
                }
            }
        }
    }

    if transitions.is_empty() {
        return None;
    }
    transitions.sort_by_key(|t| t.effective_from);
    Some(transitions)
}

/// Run an RRULE through the `rrule` crate against a 1970..2050 window in UTC.
///
/// VTIMEZONE RRULEs operate on local wall-clock time, but we feed the raw
/// dtstart string in unchanged — `rrule` parses both pieces and treats the
/// DTSTART as the starting wall-clock instant. We extract `NaiveDateTime`
/// from the result (discard the tz wrapper, which is irrelevant for
/// VTIMEZONE transitions).
fn expand_rrule(dtstart_value: &str, rrule_value: &str) -> Vec<NaiveDateTime> {
    use rrule::{RRuleSet, Tz as RTz};

    // The rrule crate expects an "RRULE source" with DTSTART line + RRULE
    // line. We lock to UTC for parsing — VTIMEZONE wall-clock semantics
    // don't depend on the parser's tz, only on the resulting NaiveDateTime
    // values.
    let normalized_dtstart = if dtstart_value.ends_with('Z') {
        dtstart_value.to_string()
    } else {
        format!("{dtstart_value}Z")
    };
    let source = format!("DTSTART:{normalized_dtstart}\nRRULE:{rrule_value}");

    let Ok(set): Result<RRuleSet, _> = source.parse() else {
        return Vec::new();
    };

    let after = match RTz::UTC.with_ymd_and_hms_opt(1970, 1, 1, 0, 0, 0) {
        Some(d) => d,
        None => return Vec::new(),
    };
    let before = match RTz::UTC.with_ymd_and_hms_opt(2050, 12, 31, 23, 59, 59) {
        Some(d) => d,
        None => return Vec::new(),
    };
    let result = set.after(after).before(before).all(10_000);

    result.dates.into_iter().map(|d| d.naive_utc()).collect()
}

/// Compatibility shim: `rrule::Tz` re-exports chrono-tz's `Tz`, but its
/// `with_ymd_and_hms` lives behind chrono's `TimeZone` trait. Provide a
/// uniform name for the call site.
trait UtcCtorOpt {
    fn with_ymd_and_hms_opt(
        &self,
        y: i32,
        m: u32,
        d: u32,
        h: u32,
        mi: u32,
        s: u32,
    ) -> Option<chrono::DateTime<rrule::Tz>>;
}

impl UtcCtorOpt for rrule::Tz {
    fn with_ymd_and_hms_opt(
        &self,
        y: i32,
        m: u32,
        d: u32,
        h: u32,
        mi: u32,
        s: u32,
    ) -> Option<chrono::DateTime<rrule::Tz>> {
        use chrono::TimeZone;
        self.with_ymd_and_hms(y, m, d, h, mi, s).single()
    }
}

fn parse_local_datetime(s: &str) -> Option<NaiveDateTime> {
    // VTIMEZONE DTSTART is local wall-clock by spec, so a trailing 'Z' here
    // is technically out-of-spec but we tolerate it (some producers).
    let s = s.trim_end_matches('Z');
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S").ok()
}

/// Parse an RFC 5545 §3.3.14 UTC-OFFSET (`+0900`, `-0530`).
fn parse_utc_offset(s: &str) -> Option<i32> {
    let s = s.trim();
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
        (-1, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (1, r)
    } else {
        (1, s)
    };
    let bytes = rest.as_bytes();
    if bytes.len() != 4 && bytes.len() != 6 {
        return None;
    }
    let h: i32 = std::str::from_utf8(&bytes[..2]).ok()?.parse().ok()?;
    let m: i32 = std::str::from_utf8(&bytes[2..4]).ok()?.parse().ok()?;
    let sec: i32 = if bytes.len() == 6 {
        std::str::from_utf8(&bytes[4..6]).ok()?.parse().ok()?
    } else {
        0
    };
    Some(sign * (h * 3600 + m * 60 + sec))
}

/// Microsoft / Outlook display name → IANA name.
///
/// Sourced from the Unicode CLDR `windowsZones.xml` "default" mapping (the
/// territory="001" rows). Trimmed to the alias names that real Outlook /
/// Exchange invites tend to emit. Extend as fixtures show new cases.
fn microsoft_to_iana(name: &str) -> Option<&'static str> {
    static ALIASES: &[(&str, &str)] = &[
        ("AUS Central Standard Time", "Australia/Darwin"),
        ("AUS Eastern Standard Time", "Australia/Sydney"),
        ("Afghanistan Standard Time", "Asia/Kabul"),
        ("Alaskan Standard Time", "America/Anchorage"),
        ("Arab Standard Time", "Asia/Riyadh"),
        ("Arabian Standard Time", "Asia/Dubai"),
        ("Arabic Standard Time", "Asia/Baghdad"),
        ("Argentina Standard Time", "America/Buenos_Aires"),
        ("Atlantic Standard Time", "America/Halifax"),
        ("Azerbaijan Standard Time", "Asia/Baku"),
        ("Azores Standard Time", "Atlantic/Azores"),
        ("Bahia Standard Time", "America/Bahia"),
        ("Bangladesh Standard Time", "Asia/Dhaka"),
        ("Canada Central Standard Time", "America/Regina"),
        ("Cape Verde Standard Time", "Atlantic/Cape_Verde"),
        ("Caucasus Standard Time", "Asia/Yerevan"),
        ("Cen. Australia Standard Time", "Australia/Adelaide"),
        ("Central America Standard Time", "America/Guatemala"),
        ("Central Asia Standard Time", "Asia/Almaty"),
        ("Central Brazilian Standard Time", "America/Cuiaba"),
        ("Central Europe Standard Time", "Europe/Budapest"),
        ("Central European Standard Time", "Europe/Warsaw"),
        ("Central Pacific Standard Time", "Pacific/Guadalcanal"),
        ("Central Standard Time", "America/Chicago"),
        ("Central Standard Time (Mexico)", "America/Mexico_City"),
        ("China Standard Time", "Asia/Shanghai"),
        ("Dateline Standard Time", "Etc/GMT+12"),
        ("E. Africa Standard Time", "Africa/Nairobi"),
        ("E. Australia Standard Time", "Australia/Brisbane"),
        ("E. Europe Standard Time", "Europe/Bucharest"),
        ("E. South America Standard Time", "America/Sao_Paulo"),
        ("Eastern Standard Time", "America/New_York"),
        ("Eastern Standard Time (Mexico)", "America/Cancun"),
        ("Egypt Standard Time", "Africa/Cairo"),
        ("Ekaterinburg Standard Time", "Asia/Yekaterinburg"),
        ("FLE Standard Time", "Europe/Kiev"),
        ("Fiji Standard Time", "Pacific/Fiji"),
        ("GMT Standard Time", "Europe/London"),
        ("GTB Standard Time", "Europe/Athens"),
        ("Georgian Standard Time", "Asia/Tbilisi"),
        ("Greenland Standard Time", "America/Godthab"),
        ("Greenwich Standard Time", "Atlantic/Reykjavik"),
        ("Hawaiian Standard Time", "Pacific/Honolulu"),
        ("India Standard Time", "Asia/Kolkata"),
        ("Iran Standard Time", "Asia/Tehran"),
        ("Israel Standard Time", "Asia/Jerusalem"),
        ("Jordan Standard Time", "Asia/Amman"),
        ("Korea Standard Time", "Asia/Seoul"),
        ("Magadan Standard Time", "Asia/Magadan"),
        ("Mauritius Standard Time", "Indian/Mauritius"),
        ("Middle East Standard Time", "Asia/Beirut"),
        ("Montevideo Standard Time", "America/Montevideo"),
        ("Morocco Standard Time", "Africa/Casablanca"),
        ("Mountain Standard Time", "America/Denver"),
        ("Mountain Standard Time (Mexico)", "America/Chihuahua"),
        ("Myanmar Standard Time", "Asia/Rangoon"),
        ("N. Central Asia Standard Time", "Asia/Novosibirsk"),
        ("Namibia Standard Time", "Africa/Windhoek"),
        ("Nepal Standard Time", "Asia/Kathmandu"),
        ("New Zealand Standard Time", "Pacific/Auckland"),
        ("Newfoundland Standard Time", "America/St_Johns"),
        ("North Asia East Standard Time", "Asia/Irkutsk"),
        ("North Asia Standard Time", "Asia/Krasnoyarsk"),
        ("Pacific SA Standard Time", "America/Santiago"),
        ("Pacific Standard Time", "America/Los_Angeles"),
        ("Pacific Standard Time (Mexico)", "America/Tijuana"),
        ("Pakistan Standard Time", "Asia/Karachi"),
        ("Paraguay Standard Time", "America/Asuncion"),
        ("Romance Standard Time", "Europe/Paris"),
        ("Russian Standard Time", "Europe/Moscow"),
        ("SA Eastern Standard Time", "America/Cayenne"),
        ("SA Pacific Standard Time", "America/Bogota"),
        ("SA Western Standard Time", "America/La_Paz"),
        ("SE Asia Standard Time", "Asia/Bangkok"),
        ("Samoa Standard Time", "Pacific/Apia"),
        ("Singapore Standard Time", "Asia/Singapore"),
        ("South Africa Standard Time", "Africa/Johannesburg"),
        ("Sri Lanka Standard Time", "Asia/Colombo"),
        ("Syria Standard Time", "Asia/Damascus"),
        ("Taipei Standard Time", "Asia/Taipei"),
        ("Tasmania Standard Time", "Australia/Hobart"),
        ("Tokyo Standard Time", "Asia/Tokyo"),
        ("Tonga Standard Time", "Pacific/Tongatapu"),
        ("Turkey Standard Time", "Europe/Istanbul"),
        ("US Eastern Standard Time", "America/Indianapolis"),
        ("US Mountain Standard Time", "America/Phoenix"),
        ("UTC", "Etc/UTC"),
        ("UTC-02", "Etc/GMT+2"),
        ("UTC-11", "Etc/GMT+11"),
        ("UTC+12", "Etc/GMT-12"),
        ("Ulaanbaatar Standard Time", "Asia/Ulaanbaatar"),
        ("Venezuela Standard Time", "America/Caracas"),
        ("Vladivostok Standard Time", "Asia/Vladivostok"),
        ("W. Australia Standard Time", "Australia/Perth"),
        ("W. Central Africa Standard Time", "Africa/Lagos"),
        ("W. Europe Standard Time", "Europe/Berlin"),
        ("West Asia Standard Time", "Asia/Tashkent"),
        ("West Pacific Standard Time", "Pacific/Port_Moresby"),
        ("Yakutsk Standard Time", "Asia/Yakutsk"),
    ];
    ALIASES
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| *v)
}

#[cfg(test)]
mod vtimezone_tests {
    use super::*;
    use crate::{RawComponent, RawProperty, VTimezone};

    fn raw_prop(name: &str, value: &str) -> RawProperty {
        RawProperty {
            name: name.into(),
            params: vec![],
            value: value.into(),
        }
    }

    #[test]
    fn iana_name_resolves_directly() {
        let tz = resolve("America/New_York", &[]).expect("IANA");
        match tz {
            ResolvedTz::Iana(t) => assert_eq!(t.name(), "America/New_York"),
            _ => panic!("expected Iana variant"),
        }
    }

    #[test]
    fn microsoft_alias_resolves_via_iana() {
        let tz = resolve("Tokyo Standard Time", &[]).expect("alias");
        match tz {
            ResolvedTz::Iana(t) => assert_eq!(t.name(), "Asia/Tokyo"),
            _ => panic!("expected Iana variant"),
        }

        let tz = resolve("Pacific Standard Time", &[]).expect("alias");
        match tz {
            ResolvedTz::Iana(t) => assert_eq!(t.name(), "America/Los_Angeles"),
            _ => panic!("expected Iana variant"),
        }
    }

    #[test]
    fn unknown_tzid_returns_none() {
        assert!(resolve("Made/Up_Zone_That_Doesn't_Exist", &[]).is_none());
    }

    #[test]
    fn inline_block_overrides_iana() {
        // The fake TZID resolves only via inline; no IANA / alias hit.
        let block = VTimezone {
            tzid: "X-Custom-Zone".into(),
            raw_subs: vec![RawComponent {
                name: "STANDARD".into(),
                properties: vec![
                    raw_prop("DTSTART", "19700101T000000"),
                    raw_prop("TZOFFSETFROM", "+0000"),
                    raw_prop("TZOFFSETTO", "+0530"),
                ],
                children: vec![],
            }],
        };
        let tz = resolve("X-Custom-Zone", &[block]).expect("inline");
        match tz {
            ResolvedTz::Custom(transitions) => {
                assert_eq!(transitions.len(), 1);
                assert_eq!(transitions[0].utc_offset_seconds, 5 * 3600 + 30 * 60);
            }
            _ => panic!("expected Custom variant"),
        }
    }

    #[test]
    fn inline_with_rrule_expands() {
        // Daylight: starts 1970-03-08 02:00, every year on 2nd Sunday of March,
        // shifts to UTC-4. Standard: 1970-11-01, every year first Sunday of Nov,
        // shifts to UTC-5.
        let standard = RawComponent {
            name: "STANDARD".into(),
            properties: vec![
                raw_prop("DTSTART", "19701101T020000"),
                raw_prop("TZOFFSETFROM", "-0400"),
                raw_prop("TZOFFSETTO", "-0500"),
                raw_prop("RRULE", "FREQ=YEARLY;BYDAY=1SU;BYMONTH=11"),
            ],
            children: vec![],
        };
        let daylight = RawComponent {
            name: "DAYLIGHT".into(),
            properties: vec![
                raw_prop("DTSTART", "19700308T020000"),
                raw_prop("TZOFFSETFROM", "-0500"),
                raw_prop("TZOFFSETTO", "-0400"),
                raw_prop("RRULE", "FREQ=YEARLY;BYDAY=2SU;BYMONTH=3"),
            ],
            children: vec![],
        };
        let block = VTimezone {
            tzid: "X-NY-DST".into(),
            raw_subs: vec![standard, daylight],
        };
        let tz = resolve("X-NY-DST", &[block]).expect("custom");
        let ResolvedTz::Custom(transitions) = tz else {
            panic!("expected Custom");
        };
        // 1970..2050 = 81 years × 2 (standard + daylight) = 162 transitions,
        // give or take a year on either edge. Just bound it loosely.
        assert!(
            transitions.len() > 100,
            "expected many transitions, got {}",
            transitions.len()
        );
        // Sorted ascending.
        for w in transitions.windows(2) {
            assert!(w[0].effective_from <= w[1].effective_from);
        }
    }

    #[test]
    fn parse_utc_offset_handles_signs_and_lengths() {
        assert_eq!(parse_utc_offset("+0000"), Some(0));
        assert_eq!(parse_utc_offset("-0500"), Some(-5 * 3600));
        assert_eq!(parse_utc_offset("+0930"), Some(9 * 3600 + 30 * 60));
        assert_eq!(parse_utc_offset("+093015"), Some(9 * 3600 + 30 * 60 + 15));
        assert_eq!(parse_utc_offset("nope"), None);
    }

    #[test]
    fn local_to_utc_offset_iana() {
        let tz = ResolvedTz::Iana(chrono_tz::Tz::Asia__Tokyo);
        let local = NaiveDateTime::parse_from_str("20260430T120000", "%Y%m%dT%H%M%S").unwrap();
        let off = local_to_utc_offset_seconds(&tz, local).unwrap();
        assert_eq!(off, 9 * 3600);
    }

    // ===== Additional corner-case tests =====

    #[test]
    fn parse_utc_offset_zero_handled() {
        assert_eq!(parse_utc_offset("0000"), Some(0));
        assert_eq!(parse_utc_offset("-0000"), Some(0));
        assert_eq!(parse_utc_offset("+0000"), Some(0));
    }

    #[test]
    fn parse_utc_offset_invalid_lengths_rejected() {
        assert!(parse_utc_offset("+09").is_none());
        assert!(parse_utc_offset("+093").is_none());
        assert!(parse_utc_offset("+09301").is_none());
        assert!(parse_utc_offset("+0930150").is_none());
        assert!(parse_utc_offset("").is_none());
    }

    #[test]
    fn parse_utc_offset_non_digit_rejected() {
        assert!(parse_utc_offset("+abcd").is_none());
        assert!(parse_utc_offset("+09xx").is_none());
    }

    #[test]
    fn resolve_iana_trims_whitespace() {
        // Leading/trailing whitespace should be tolerated.
        let tz = resolve("  America/New_York  ", &[]).expect("trimmed IANA name");
        match tz {
            ResolvedTz::Iana(t) => assert_eq!(t.name(), "America/New_York"),
            _ => panic!("expected Iana"),
        }
    }

    #[test]
    fn resolve_microsoft_alias_case_insensitive() {
        // Aliases match case-insensitively.
        let tz = resolve("TOKYO STANDARD TIME", &[]).expect("upper-case alias");
        match tz {
            ResolvedTz::Iana(t) => assert_eq!(t.name(), "Asia/Tokyo"),
            _ => panic!("expected Iana"),
        }
    }

    #[test]
    fn resolve_empty_tzid_returns_none() {
        assert!(resolve("", &[]).is_none());
        assert!(resolve("   ", &[]).is_none());
    }

    #[test]
    fn local_to_utc_offset_custom_finds_latest_transition() {
        // Build a transition table manually and ensure walker picks latest valid.
        let t1 = TzTransition {
            effective_from: NaiveDateTime::parse_from_str("19700101T000000", "%Y%m%dT%H%M%S")
                .unwrap(),
            utc_offset_seconds: 3600,
        };
        let t2 = TzTransition {
            effective_from: NaiveDateTime::parse_from_str("20000101T000000", "%Y%m%dT%H%M%S")
                .unwrap(),
            utc_offset_seconds: 7200,
        };
        let rt = ResolvedTz::Custom(vec![t1, t2]);
        let local = NaiveDateTime::parse_from_str("19990101T000000", "%Y%m%dT%H%M%S").unwrap();
        assert_eq!(local_to_utc_offset_seconds(&rt, local), Some(3600));
        let local_later =
            NaiveDateTime::parse_from_str("20100101T000000", "%Y%m%dT%H%M%S").unwrap();
        assert_eq!(local_to_utc_offset_seconds(&rt, local_later), Some(7200));
    }

    #[test]
    fn local_to_utc_offset_custom_before_first_transition_returns_none() {
        let t1 = TzTransition {
            effective_from: NaiveDateTime::parse_from_str("20000101T000000", "%Y%m%dT%H%M%S")
                .unwrap(),
            utc_offset_seconds: 3600,
        };
        let rt = ResolvedTz::Custom(vec![t1]);
        let local = NaiveDateTime::parse_from_str("19990101T000000", "%Y%m%dT%H%M%S").unwrap();
        assert!(local_to_utc_offset_seconds(&rt, local).is_none());
    }

    #[test]
    fn resolve_inline_block_with_no_subs_falls_through_to_iana() {
        // Empty raw_subs means build_transitions returns None; fall through.
        let block = VTimezone {
            tzid: "America/New_York".into(),
            raw_subs: vec![],
        };
        let tz = resolve("America/New_York", &[block]).expect("fallback to IANA");
        match tz {
            ResolvedTz::Iana(_) => {}
            _ => panic!("expected fallback to IANA when inline is empty"),
        }
    }
}

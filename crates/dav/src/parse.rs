//! Pure parsing helpers: iCalendar / vCard line scrapers + the WebDAV
//! `Depth` header parser. No async, no I/O.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

/// Parse the WebDAV `Depth` header value.
///
/// Returns `0`, `1`, or `u32::MAX` (for `infinity`); defaults to `1` per
/// RFC 4918 §10.2 when the header is absent or unparseable.
pub fn parse_depth(value: Option<&str>) -> u32 {
    value
        .and_then(|s| match s.trim() {
            "0" => Some(0),
            "1" => Some(1),
            "infinity" => Some(u32::MAX),
            _ => None,
        })
        .unwrap_or(1)
}

/// Extract a single iCalendar property value by name.
///
/// Handles both `FIELD:value` and `FIELD;param=...:value`. Returns the empty
/// string when the field isn't present (matches the reference implementation
/// — callers distinguish "empty" via `.is_empty()`).
pub fn extract_ical_field(ical: &str, field: &str) -> String {
    for line in ical.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            if let Some(value) = rest.strip_prefix(':') {
                return value.trim().to_string();
            }
            if rest.starts_with(';')
                && let Some(pos) = rest.find(':')
            {
                return rest[pos + 1..].trim().to_string();
            }
        }
    }
    String::new()
}

/// Extract an iCalendar DATE-TIME property and parse it as UTC.
///
/// Accepts the three forms RFC 5545 emits in practice:
/// - `20240101T120000Z` — UTC
/// - `20240101T120000` — floating, treated as UTC
/// - `20240101` — DATE only, treated as 00:00:00 UTC
pub fn extract_ical_datetime(ical: &str, field: &str) -> Option<DateTime<Utc>> {
    let value = extract_ical_field(ical, field);
    if value.is_empty() {
        return None;
    }

    if let Ok(dt) = NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ") {
        return Some(dt.and_utc());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%S") {
        return Some(dt.and_utc());
    }
    if let Ok(d) = NaiveDate::parse_from_str(&value, "%Y%m%d") {
        return d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc());
    }
    None
}

/// Extract a vCard property value by name. Same shape as
/// [`extract_ical_field`]; kept as a separate function for callsite clarity.
pub fn extract_vcard_field(vcard: &str, field: &str) -> String {
    extract_ical_field(vcard, field)
}

/// Extract every UID from `<D:href>` / `<href>` elements in a multiget request
/// body whose href ends with `suffix` (e.g. `.ics` for calendar-multiget,
/// `.vcf` for addressbook-multiget). UIDs are returned in the order they
/// appear in the body.
///
/// This is intentionally a string scrape rather than a real XML parse — DAV
/// multiget bodies in practice are small and the formats clients emit are
/// well-known.
pub fn extract_multiget_uids(body: &str, suffix: &str) -> Vec<String> {
    let mut uids = Vec::new();
    for tag in ["<D:href>", "<href>"] {
        let close = if tag == "<D:href>" {
            "</D:href>"
        } else {
            "</href>"
        };
        let open_len = tag.len();
        for href_start in body.match_indices(tag).map(|(i, _)| i) {
            let rest = &body[href_start + open_len..];
            if let Some(end) = rest.find(close) {
                let href = rest[..end].trim();
                if let Some(uid) = href.strip_suffix(suffix).and_then(|s| s.rsplit('/').next()) {
                    uids.push(uid.to_string());
                }
            }
        }
    }
    uids
}

/// [`extract_multiget_uids`], decoded: the UIDs a multiget asks for, as the
/// store names them. A malformed escape is the client's error.
pub fn multiget_uids(body: &str, suffix: &str) -> Result<Vec<String>, crate::error::DavError> {
    extract_multiget_uids(body, suffix)
        .iter()
        .map(|raw| {
            percent_decode(raw)
                .ok_or_else(|| crate::error::DavError::BadRequest(format!("href {raw:?}")))
        })
        .collect()
}

/// Percent-encode one URL path segment (RFC 3986 unreserved characters pass
/// through). Used for collection names and resource UIDs in emitted hrefs.
pub fn encode_segment(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'@' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Decode one percent-encoded URL path segment. `None` when an escape is
/// malformed or the decoded bytes are not UTF-8 — the segment came from a
/// client and cannot name anything.
pub fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_defaults_to_one_when_missing() {
        assert_eq!(parse_depth(None), 1);
        assert_eq!(parse_depth(Some("garbage")), 1);
    }

    #[test]
    fn depth_zero_and_one_parsed_literally() {
        assert_eq!(parse_depth(Some("0")), 0);
        assert_eq!(parse_depth(Some("1")), 1);
    }

    #[test]
    fn depth_infinity_yields_u32_max() {
        assert_eq!(parse_depth(Some("infinity")), u32::MAX);
    }

    #[test]
    fn ical_field_simple() {
        let ical = "BEGIN:VEVENT\nSUMMARY:Team Meeting\nEND:VEVENT";
        assert_eq!(extract_ical_field(ical, "SUMMARY"), "Team Meeting");
    }

    #[test]
    fn ical_field_with_params() {
        let ical = "BEGIN:VEVENT\nDTSTART;TZID=US/Eastern:20240101T120000\nEND:VEVENT";
        assert_eq!(extract_ical_field(ical, "DTSTART"), "20240101T120000");
    }

    #[test]
    fn ical_field_missing_returns_empty() {
        assert_eq!(extract_ical_field("SUMMARY:x", "DESCRIPTION"), "");
    }

    #[test]
    fn ical_datetime_utc_form() {
        let dt = extract_ical_datetime("DTSTART:20240315T100000Z", "DTSTART").unwrap();
        assert_eq!(dt.to_rfc3339(), "2024-03-15T10:00:00+00:00");
    }

    #[test]
    fn ical_datetime_floating_form() {
        let dt = extract_ical_datetime("DTSTART:20240315T100000", "DTSTART").unwrap();
        assert_eq!(dt.to_rfc3339(), "2024-03-15T10:00:00+00:00");
    }

    #[test]
    fn ical_datetime_date_only() {
        let dt = extract_ical_datetime("DTSTART;VALUE=DATE:20240315", "DTSTART").unwrap();
        assert_eq!(dt.to_rfc3339(), "2024-03-15T00:00:00+00:00");
    }

    #[test]
    fn ical_datetime_missing_yields_none() {
        assert!(extract_ical_datetime("SUMMARY:x", "DTSTART").is_none());
    }

    #[test]
    fn vcard_field_simple() {
        let vcard = "BEGIN:VCARD\nFN:John Doe\nEMAIL:john@example.com\nEND:VCARD";
        assert_eq!(extract_vcard_field(vcard, "FN"), "John Doe");
        assert_eq!(extract_vcard_field(vcard, "EMAIL"), "john@example.com");
    }

    #[test]
    fn vcard_field_with_params() {
        let vcard = "EMAIL;TYPE=WORK:john@company.com";
        assert_eq!(extract_vcard_field(vcard, "EMAIL"), "john@company.com");
    }

    #[test]
    fn multiget_extracts_ics_uids_in_order() {
        let body = "<D:href>/dav/calendars/u/c/abc.ics</D:href>\n\
                    <D:href>/dav/calendars/u/c/def.ics</D:href>";
        let uids = extract_multiget_uids(body, ".ics");
        assert_eq!(uids, vec!["abc".to_string(), "def".to_string()]);
    }

    #[test]
    fn multiget_handles_unprefixed_href_tag() {
        let body = "<href>/dav/addressbooks/u/b/xyz.vcf</href>";
        let uids = extract_multiget_uids(body, ".vcf");
        assert_eq!(uids, vec!["xyz".to_string()]);
    }

    #[test]
    fn multiget_skips_hrefs_without_correct_suffix() {
        let body = "<D:href>/dav/calendars/u/c/abc.ics</D:href>\n\
                    <D:href>/dav/calendars/u/c/wrong.vcf</D:href>";
        let uids = extract_multiget_uids(body, ".ics");
        assert_eq!(uids, vec!["abc".to_string()]);
    }

    #[test]
    fn segment_encoding_passes_unreserved_and_at_sign() {
        assert_eq!(encode_segment("Hello123-_.~"), "Hello123-_.~");
        assert_eq!(encode_segment("alice@example.com"), "alice@example.com");
    }

    #[test]
    fn segment_encoding_escapes_the_rest_bytewise() {
        assert_eq!(encode_segment("a b/c?"), "a%20b%2Fc%3F");
        assert_eq!(encode_segment("日"), "%E6%97%A5");
    }

    #[test]
    fn percent_decode_round_trips_encode_segment() {
        for s in ["a b", "日本", "x/y?z", "plain"] {
            assert_eq!(percent_decode(&encode_segment(s)).as_deref(), Some(s));
        }
    }

    #[test]
    fn percent_decode_rejects_malformed_escapes() {
        assert_eq!(percent_decode("%4"), None);
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%FF"), None);
    }
}

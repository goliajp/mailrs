//! MTA-STS TXT record parser (RFC 8461 §3.1).
//!
//! `_mta-sts.<domain>` returns a TXT record like:
//!
//! ```text
//! v=STSv1; id=20200101T000000Z
//! ```
//!
//! The `id` is an opaque string ≤ 32 chars that changes when the
//! domain's STS policy changes. Receivers cache the parsed policy
//! per id and re-fetch only when the id moves.

use compact_str::CompactString;

use crate::error::MtaStsError;

/// Maximum length of the `id` field per RFC 8461 §3.1.
pub const MAX_ID_LEN: usize = 32;

/// Parsed `_mta-sts.<domain>` TXT record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StsRecord {
    /// `id=` value. Opaque ≤ 32 chars; treat as a cache key.
    ///
    /// **v2 change**: `CompactString` — RFC caps `id` at 32 chars; the
    /// 24-byte inline buffer covers the overwhelmingly common case
    /// (typical IDs are timestamp-shaped like `20200101T000000Z`).
    pub id: CompactString,
}

impl StsRecord {
    /// Parse an MTA-STS TXT record. Tolerates leading whitespace,
    /// case-insensitive tag names, and unknown tags (forward-compat).
    pub fn parse(txt: &str) -> Result<Self, MtaStsError> {
        let mut saw_version = false;
        let mut id: Option<CompactString> = None;
        for tag in txt.split(';') {
            let tag = tag.trim();
            if tag.is_empty() {
                continue;
            }
            let (name, value) = match tag.split_once('=') {
                Some(pair) => pair,
                None => continue,
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            match name.as_str() {
                "v" => {
                    if !value.eq_ignore_ascii_case("STSv1") {
                        return Err(MtaStsError::NotAnStsRecord);
                    }
                    saw_version = true;
                }
                "id" => {
                    if value.len() > MAX_ID_LEN {
                        return Err(MtaStsError::IdTooLong(value.len()));
                    }
                    id = Some(CompactString::new(value));
                }
                _ => {} // forward-compat: unknown tags skipped per RFC 8461 §3.1
            }
        }
        if !saw_version {
            return Err(MtaStsError::NotAnStsRecord);
        }
        Ok(Self {
            id: id.ok_or(MtaStsError::MissingId)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let r = StsRecord::parse("v=STSv1; id=20200101T000000Z").unwrap();
        assert_eq!(r.id, "20200101T000000Z");
    }

    #[test]
    fn parse_tolerates_leading_whitespace() {
        let r = StsRecord::parse("   v=STSv1; id=abc123").unwrap();
        assert_eq!(r.id, "abc123");
    }

    #[test]
    fn parse_case_insensitive_tag_names() {
        let r = StsRecord::parse("V=STSv1; ID=xyz").unwrap();
        assert_eq!(r.id, "xyz");
    }

    #[test]
    fn parse_case_insensitive_version_value() {
        let r = StsRecord::parse("v=stsv1; id=abc").unwrap();
        assert_eq!(r.id, "abc");
    }

    #[test]
    fn parse_ignores_unknown_tags_forward_compat() {
        let r = StsRecord::parse("v=STSv1; id=abc; future=value").unwrap();
        assert_eq!(r.id, "abc");
    }

    #[test]
    fn parse_rejects_missing_version() {
        let r = StsRecord::parse("id=abc123");
        assert!(matches!(r, Err(MtaStsError::NotAnStsRecord)));
    }

    #[test]
    fn parse_rejects_wrong_version() {
        let r = StsRecord::parse("v=STSv2; id=abc");
        assert!(matches!(r, Err(MtaStsError::NotAnStsRecord)));
    }

    #[test]
    fn parse_rejects_missing_id() {
        let r = StsRecord::parse("v=STSv1");
        assert!(matches!(r, Err(MtaStsError::MissingId)));
    }

    #[test]
    fn parse_rejects_id_too_long() {
        let long_id = "x".repeat(33);
        let txt = format!("v=STSv1; id={long_id}");
        let r = StsRecord::parse(&txt);
        assert!(matches!(r, Err(MtaStsError::IdTooLong(33))));
    }

    #[test]
    fn parse_handles_extra_whitespace_around_equals() {
        let r = StsRecord::parse("v = STSv1 ; id = xyz123").unwrap();
        assert_eq!(r.id, "xyz123");
    }

    #[test]
    fn parse_handles_trailing_semicolon() {
        let r = StsRecord::parse("v=STSv1; id=abc;").unwrap();
        assert_eq!(r.id, "abc");
    }
}

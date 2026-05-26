use super::unquote;

/// Parsed IMAP SEARCH key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchKey {
    /// `ALL` — match every message in the mailbox.
    All,
    /// `SEEN` — messages with the `\Seen` flag.
    Seen,
    /// `UNSEEN` — messages without `\Seen`.
    Unseen,
    /// `FLAGGED` — messages with `\Flagged`.
    Flagged,
    /// `UNFLAGGED` — messages without `\Flagged`.
    Unflagged,
    /// `ANSWERED` — messages with `\Answered`.
    Answered,
    /// `UNANSWERED` — messages without `\Answered`.
    Unanswered,
    /// `DELETED` — messages with `\Deleted`.
    Deleted,
    /// `UNDELETED` — messages without `\Deleted`.
    Undeleted,
    /// `DRAFT` — messages with `\Draft`.
    Draft,
    /// `UNDRAFT` — messages without `\Draft`.
    Undraft,
    /// `RECENT` — messages with `\Recent` (per-session).
    Recent,
    /// `FROM <string>` — substring match on the From header.
    From(String),
    /// `TO <string>` — substring match on the To header.
    To(String),
    /// `SUBJECT <string>` — substring match on the Subject header.
    Subject(String),
    /// `TEXT <string>` — substring match on headers + body.
    Text(String),
    /// `BODY <string>` — substring match on body only.
    Body(String),
    /// `SINCE <date>` — internal date on or after (epoch seconds).
    Since(i64),
    /// `BEFORE <date>` — internal date strictly before (epoch seconds).
    Before(i64),
    /// `ON <date>` — internal date matches that day (epoch seconds, start of day).
    On(i64),
    /// `UID <sequence-set>` — match specific UIDs.
    Uid(String),
}

/// parse IMAP SEARCH criteria string into a list of search keys
///
/// supports AND-combination (multiple keys all must match).
/// unknown tokens are silently skipped to stay compatible.
pub fn parse_search_criteria(criteria: &str) -> Vec<SearchKey> {
    let criteria = criteria.trim();
    if criteria.is_empty() {
        return vec![SearchKey::All];
    }

    let mut keys = Vec::new();
    let tokens = tokenize_search(criteria);
    let mut i = 0;

    while i < tokens.len() {
        // Stack-buffer uppercase: SEARCH keywords are bounded ASCII
        // (longest is `UNANSWERED` at 10 bytes). Fits in 16; tokens
        // exceeding 16 are non-keywords (quoted strings / numbers /
        // unknown extensions) and fall through to the default arm.
        let tok = tokens[i].as_bytes();
        if tok.len() > 16 {
            i += 1;
            continue;
        }
        let mut buf = [0u8; 16];
        for (j, &b) in tok.iter().enumerate() {
            buf[j] = b.to_ascii_uppercase();
        }
        let kw = &buf[..tok.len()];
        match kw {
            b"ALL" => keys.push(SearchKey::All),
            b"SEEN" => keys.push(SearchKey::Seen),
            b"UNSEEN" => keys.push(SearchKey::Unseen),
            b"FLAGGED" => keys.push(SearchKey::Flagged),
            b"UNFLAGGED" => keys.push(SearchKey::Unflagged),
            b"ANSWERED" => keys.push(SearchKey::Answered),
            b"UNANSWERED" => keys.push(SearchKey::Unanswered),
            b"DELETED" => keys.push(SearchKey::Deleted),
            b"UNDELETED" => keys.push(SearchKey::Undeleted),
            b"DRAFT" => keys.push(SearchKey::Draft),
            b"UNDRAFT" => keys.push(SearchKey::Undraft),
            b"RECENT" => keys.push(SearchKey::Recent),
            b"FROM" if i + 1 < tokens.len() => {
                i += 1;
                keys.push(SearchKey::From(unquote(&tokens[i])));
            }
            b"TO" if i + 1 < tokens.len() => {
                i += 1;
                keys.push(SearchKey::To(unquote(&tokens[i])));
            }
            b"SUBJECT" if i + 1 < tokens.len() => {
                i += 1;
                keys.push(SearchKey::Subject(unquote(&tokens[i])));
            }
            b"TEXT" if i + 1 < tokens.len() => {
                i += 1;
                keys.push(SearchKey::Text(unquote(&tokens[i])));
            }
            b"BODY" if i + 1 < tokens.len() => {
                i += 1;
                keys.push(SearchKey::Body(unquote(&tokens[i])));
            }
            b"SINCE" if i + 1 < tokens.len() => {
                i += 1;
                if let Some(ts) = parse_imap_date(&tokens[i]) {
                    keys.push(SearchKey::Since(ts));
                }
            }
            b"BEFORE" if i + 1 < tokens.len() => {
                i += 1;
                if let Some(ts) = parse_imap_date(&tokens[i]) {
                    keys.push(SearchKey::Before(ts));
                }
            }
            b"ON" if i + 1 < tokens.len() => {
                i += 1;
                if let Some(ts) = parse_imap_date(&tokens[i]) {
                    keys.push(SearchKey::On(ts));
                }
            }
            b"UID" if i + 1 < tokens.len() => {
                i += 1;
                keys.push(SearchKey::Uid(tokens[i].clone()));
            }
            _ => {} // skip unknown tokens (CHARSET / UTF-8 / extensions)
        }
        i += 1;
    }

    if keys.is_empty() {
        keys.push(SearchKey::All);
    }
    keys
}

/// tokenize search criteria, respecting quoted strings
fn tokenize_search(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                if in_quote {
                    // end of quoted string, push with quotes for unquote()
                    tokens.push(format!("\"{current}\""));
                    current.clear();
                    in_quote = false;
                } else {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                    in_quote = true;
                }
            }
            ' ' if !in_quote => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// parse IMAP date format: d-Mon-yyyy (e.g. "1-Jan-2024" or "01-Jan-2024")
/// returns epoch seconds (start of day UTC)
fn parse_imap_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let day: u32 = parts[0].parse().ok()?;
    // Month is exactly 3 ASCII letters per RFC 3501 — byte-level compare
    // avoids the per-call `to_uppercase()` String alloc.
    let mb = parts[1].as_bytes();
    if mb.len() != 3 {
        return None;
    }
    let month = match [
        mb[0].to_ascii_uppercase(),
        mb[1].to_ascii_uppercase(),
        mb[2].to_ascii_uppercase(),
    ] {
        [b'J', b'A', b'N'] => 1,
        [b'F', b'E', b'B'] => 2,
        [b'M', b'A', b'R'] => 3,
        [b'A', b'P', b'R'] => 4,
        [b'M', b'A', b'Y'] => 5,
        [b'J', b'U', b'N'] => 6,
        [b'J', b'U', b'L'] => 7,
        [b'A', b'U', b'G'] => 8,
        [b'S', b'E', b'P'] => 9,
        [b'O', b'C', b'T'] => 10,
        [b'N', b'O', b'V'] => 11,
        [b'D', b'E', b'C'] => 12,
        _ => return None,
    };
    let year: i64 = parts[2].parse().ok()?;

    // simple date-to-epoch conversion (UTC, no leap second handling)
    let mut days: i64 = 0;
    // years since epoch
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    // months in current year
    let days_in_months = [
        31,
        28 + if is_leap_year(year) { 1 } else { 0 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for d in days_in_months.iter().take((month - 1) as usize) {
        days += *d as i64;
    }
    days += day as i64 - 1;
    Some(days * 86400)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SearchCriteria parsing tests ---

    #[test]
    fn search_criteria_empty_returns_all() {
        let keys = parse_search_criteria("");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_all() {
        let keys = parse_search_criteria("ALL");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_flag_keys() {
        assert_eq!(parse_search_criteria("SEEN"), vec![SearchKey::Seen]);
        assert_eq!(parse_search_criteria("UNSEEN"), vec![SearchKey::Unseen]);
        assert_eq!(parse_search_criteria("FLAGGED"), vec![SearchKey::Flagged]);
        assert_eq!(
            parse_search_criteria("UNFLAGGED"),
            vec![SearchKey::Unflagged]
        );
        assert_eq!(parse_search_criteria("ANSWERED"), vec![SearchKey::Answered]);
        assert_eq!(
            parse_search_criteria("UNANSWERED"),
            vec![SearchKey::Unanswered]
        );
        assert_eq!(parse_search_criteria("DELETED"), vec![SearchKey::Deleted]);
        assert_eq!(
            parse_search_criteria("UNDELETED"),
            vec![SearchKey::Undeleted]
        );
        assert_eq!(parse_search_criteria("DRAFT"), vec![SearchKey::Draft]);
        assert_eq!(parse_search_criteria("UNDRAFT"), vec![SearchKey::Undraft]);
        assert_eq!(parse_search_criteria("RECENT"), vec![SearchKey::Recent]);
    }

    #[test]
    fn search_criteria_case_insensitive() {
        assert_eq!(parse_search_criteria("unseen"), vec![SearchKey::Unseen]);
        assert_eq!(parse_search_criteria("Flagged"), vec![SearchKey::Flagged]);
    }

    #[test]
    fn search_criteria_from() {
        let keys = parse_search_criteria("FROM user@example.com");
        assert_eq!(keys, vec![SearchKey::From("user@example.com".into())]);
    }

    #[test]
    fn search_criteria_from_quoted() {
        let keys = parse_search_criteria("FROM \"John Doe\"");
        assert_eq!(keys, vec![SearchKey::From("John Doe".into())]);
    }

    #[test]
    fn search_criteria_to() {
        let keys = parse_search_criteria("TO admin@example.com");
        assert_eq!(keys, vec![SearchKey::To("admin@example.com".into())]);
    }

    #[test]
    fn search_criteria_subject() {
        let keys = parse_search_criteria("SUBJECT \"meeting notes\"");
        assert_eq!(keys, vec![SearchKey::Subject("meeting notes".into())]);
    }

    #[test]
    fn search_criteria_subject_unquoted() {
        let keys = parse_search_criteria("SUBJECT hello");
        assert_eq!(keys, vec![SearchKey::Subject("hello".into())]);
    }

    #[test]
    fn search_criteria_text() {
        let keys = parse_search_criteria("TEXT \"important update\"");
        assert_eq!(keys, vec![SearchKey::Text("important update".into())]);
    }

    #[test]
    fn search_criteria_body() {
        let keys = parse_search_criteria("BODY invoice");
        assert_eq!(keys, vec![SearchKey::Body("invoice".into())]);
    }

    #[test]
    fn search_criteria_since() {
        let keys = parse_search_criteria("SINCE 1-Jan-2024");
        // 1-Jan-2024 = 19723 days from epoch = 19723 * 86400
        assert_eq!(keys, vec![SearchKey::Since(19723 * 86400)]);
    }

    #[test]
    fn search_criteria_before() {
        let keys = parse_search_criteria("BEFORE 15-Mar-2024");
        assert_eq!(keys.len(), 1);
        assert!(matches!(keys[0], SearchKey::Before(_)));
    }

    #[test]
    fn search_criteria_on() {
        let keys = parse_search_criteria("ON 1-Feb-2024");
        assert_eq!(keys.len(), 1);
        assert!(matches!(keys[0], SearchKey::On(_)));
    }

    #[test]
    fn search_criteria_uid() {
        let keys = parse_search_criteria("UID 1:100");
        assert_eq!(keys, vec![SearchKey::Uid("1:100".into())]);
    }

    #[test]
    fn search_criteria_uid_single() {
        let keys = parse_search_criteria("UID 42");
        assert_eq!(keys, vec![SearchKey::Uid("42".into())]);
    }

    #[test]
    fn search_criteria_multiple_and() {
        let keys = parse_search_criteria("UNSEEN FROM user@example.com");
        assert_eq!(
            keys,
            vec![
                SearchKey::Unseen,
                SearchKey::From("user@example.com".into()),
            ]
        );
    }

    #[test]
    fn search_criteria_complex_combination() {
        let keys = parse_search_criteria("SINCE 1-Jan-2024 FROM user@example.com UNSEEN");
        assert_eq!(keys.len(), 3);
        assert!(matches!(keys[0], SearchKey::Since(_)));
        assert_eq!(keys[1], SearchKey::From("user@example.com".into()));
        assert_eq!(keys[2], SearchKey::Unseen);
    }

    #[test]
    fn search_criteria_skips_charset() {
        // CHARSET UTF-8 is commonly sent by clients, should be skipped
        let keys = parse_search_criteria("CHARSET UTF-8 UNSEEN");
        assert_eq!(keys, vec![SearchKey::Unseen]);
    }

    #[test]
    fn search_criteria_unknown_tokens_skipped() {
        let keys = parse_search_criteria("FOOBAR UNSEEN");
        assert_eq!(keys, vec![SearchKey::Unseen]);
    }

    #[test]
    fn search_criteria_date_parsing_jan() {
        let keys = parse_search_criteria("SINCE 1-Jan-1970");
        assert_eq!(keys, vec![SearchKey::Since(0)]);
    }

    #[test]
    fn search_criteria_date_parsing_various_months() {
        // verify all months parse without error
        for month in &[
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ] {
            let criteria = format!("SINCE 1-{month}-2024");
            let keys = parse_search_criteria(&criteria);
            assert_eq!(keys.len(), 1, "failed for month {month}");
            assert!(
                matches!(keys[0], SearchKey::Since(_)),
                "failed for month {month}"
            );
        }
    }

    #[test]
    fn search_criteria_invalid_date_skipped() {
        let keys = parse_search_criteria("SINCE not-a-date");
        // invalid date is skipped, falls back to ALL
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn tokenize_quoted_strings() {
        let tokens = tokenize_search("FROM \"John Doe\" SUBJECT \"hello world\"");
        assert_eq!(
            tokens,
            vec!["FROM", "\"John Doe\"", "SUBJECT", "\"hello world\""]
        );
    }

    #[test]
    fn tokenize_no_quotes() {
        let tokens = tokenize_search("UNSEEN FLAGGED");
        assert_eq!(tokens, vec!["UNSEEN", "FLAGGED"]);
    }

    #[test]
    fn imap_date_epoch() {
        assert_eq!(parse_imap_date("1-Jan-1970"), Some(0));
    }

    #[test]
    fn imap_date_2024() {
        // 2024-01-01 = 19723 days from epoch
        let ts = parse_imap_date("1-Jan-2024").unwrap();
        assert_eq!(ts, 19723 * 86400);
    }

    #[test]
    fn imap_date_invalid() {
        assert_eq!(parse_imap_date("invalid"), None);
        assert_eq!(parse_imap_date("1-Xyz-2024"), None);
        assert_eq!(parse_imap_date("abc-Jan-2024"), None);
    }

    #[test]
    fn imap_date_leap_year() {
        // 2024 is a leap year; 1-Mar-2024 should account for 29 days in Feb
        let feb29 = parse_imap_date("29-Feb-2024");
        assert!(feb29.is_some());
        let mar1 = parse_imap_date("1-Mar-2024").unwrap();
        assert_eq!(mar1, feb29.unwrap() + 86400);
    }

    #[test]
    fn search_criteria_from_at_end_no_value() {
        // FROM without a following token should be skipped, fall back to ALL
        let keys = parse_search_criteria("FROM");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_to_at_end_no_value() {
        let keys = parse_search_criteria("TO");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_subject_at_end_no_value() {
        let keys = parse_search_criteria("SUBJECT");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_text_at_end_no_value() {
        let keys = parse_search_criteria("TEXT");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_body_at_end_no_value() {
        let keys = parse_search_criteria("BODY");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_uid_at_end_no_value() {
        let keys = parse_search_criteria("UID");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_since_at_end_no_value() {
        let keys = parse_search_criteria("SINCE");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_before_at_end_no_value() {
        let keys = parse_search_criteria("BEFORE");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_on_at_end_no_value() {
        let keys = parse_search_criteria("ON");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_whitespace_only_returns_all() {
        let keys = parse_search_criteria("   ");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_multiple_flags_and_parameterized() {
        let keys = parse_search_criteria("UNSEEN UNDELETED SUBJECT test FROM sender@x.com");
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[0], SearchKey::Unseen);
        assert_eq!(keys[1], SearchKey::Undeleted);
        assert_eq!(keys[2], SearchKey::Subject("test".into()));
        assert_eq!(keys[3], SearchKey::From("sender@x.com".into()));
    }

    #[test]
    fn imap_date_century_non_leap_1900() {
        // 1900 is divisible by 100 but not 400 — not a leap year
        assert!(!is_leap_year(1900));
    }

    #[test]
    fn imap_date_century_leap_2000() {
        // 2000 is divisible by 400 — leap year
        assert!(is_leap_year(2000));
    }

    #[test]
    fn imap_date_non_leap_2023() {
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn imap_date_leap_2024() {
        assert!(is_leap_year(2024));
    }

    #[test]
    fn imap_date_two_digit_day() {
        let ts = parse_imap_date("15-Jun-2024");
        assert!(ts.is_some());
    }

    #[test]
    fn imap_date_missing_parts() {
        assert_eq!(parse_imap_date("1-Jan"), None);
        assert_eq!(parse_imap_date("2024"), None);
        assert_eq!(parse_imap_date(""), None);
    }

    #[test]
    fn imap_date_invalid_year() {
        assert_eq!(parse_imap_date("1-Jan-abc"), None);
    }

    #[test]
    fn imap_date_dec_31() {
        // last day of a non-leap year
        let dec31 = parse_imap_date("31-Dec-2023").unwrap();
        let jan1_next = parse_imap_date("1-Jan-2024").unwrap();
        assert_eq!(jan1_next - dec31, 86400);
    }

    #[test]
    fn tokenize_search_empty() {
        let tokens = tokenize_search("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_search_only_spaces() {
        let tokens = tokenize_search("   ");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_search_multiple_spaces_between_tokens() {
        let tokens = tokenize_search("FROM   user@example.com");
        assert_eq!(tokens, vec!["FROM", "user@example.com"]);
    }

    #[test]
    fn tokenize_search_unclosed_quote_treated_as_unquoted() {
        // unclosed quote: remaining text pushed as-is
        let tokens = tokenize_search("FROM \"unclosed");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], "FROM");
        // unclosed quote leaves the text in current buffer
        assert_eq!(tokens[1], "unclosed");
    }
}

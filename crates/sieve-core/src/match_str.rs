//! RFC 5228 §2.7 string comparison + §2.7.1 wildcard `:matches`
//! engine. Extracted from `eval.rs` so the evaluator stays under
//! the file-size limit.

use crate::ast::MatchType;

/// Match-type comparison. All comparisons are case-insensitive
/// (RFC 5228 default comparator `i;ascii-casemap`).
pub(crate) fn match_string(mt: MatchType, haystack: &str, needle: &str) -> bool {
    let h = haystack.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    match mt {
        MatchType::Is => h == n,
        MatchType::Contains => h.contains(&n),
        MatchType::Matches => glob_match(&h, &n),
    }
}

/// Tiny glob matcher: `*` matches any sequence, `?` matches one
/// char. ASCII only — sufficient for RFC 5228 `:matches` against
/// header values.
pub(crate) fn glob_match(haystack: &str, pattern: &str) -> bool {
    let h = haystack.as_bytes();
    let p = pattern.as_bytes();
    rec(h, p)
}

fn rec(h: &[u8], p: &[u8]) -> bool {
    if p.is_empty() {
        return h.is_empty();
    }
    match p[0] {
        b'*' => {
            for i in 0..=h.len() {
                if rec(&h[i..], &p[1..]) {
                    return true;
                }
            }
            false
        }
        b'?' => !h.is_empty() && rec(&h[1..], &p[1..]),
        c => !h.is_empty() && h[0] == c && rec(&h[1..], &p[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_case_insensitive() {
        assert!(match_string(MatchType::Is, "Hello", "HELLO"));
        assert!(!match_string(MatchType::Is, "Hello", "World"));
    }

    #[test]
    fn contains_substring() {
        assert!(match_string(MatchType::Contains, "spam offer here", "OFFER"));
        assert!(!match_string(MatchType::Contains, "spam offer", "newsletter"));
    }

    #[test]
    fn glob_star_anywhere() {
        assert!(glob_match("hello world", "hello*"));
        assert!(glob_match("hello world", "*world"));
        assert!(glob_match("hello world", "*lo wor*"));
        assert!(!glob_match("hello", "world*"));
    }

    #[test]
    fn glob_question_one_char() {
        assert!(glob_match("cat", "c?t"));
        assert!(!glob_match("cat", "c??t"));
        assert!(glob_match("cat", "ca?"));
        assert!(!glob_match("ca", "ca?"));
    }

    #[test]
    fn glob_star_empty_sequence() {
        assert!(glob_match("abc", "*abc"));
        assert!(glob_match("abc", "abc*"));
        assert!(glob_match("", "*"));
        assert!(glob_match("anything", "*"));
    }
}

//! RFC 5228 §2.7 string comparison + §2.7.1 wildcard `:matches`
//! engine. Extracted from `eval.rs` so the evaluator stays under
//! the file-size limit.
//!
//! All comparisons use the RFC 5228 default `i;ascii-casemap`
//! comparator — ASCII case-insensitive. The earlier implementation
//! allocated two `String`s per call (lowercasing both sides); the
//! v4 ckpt 25 rewrite keeps everything zero-alloc by going through
//! case-insensitive byte ops (`slice::eq_ignore_ascii_case`) plus
//! memchr2-anchored substring search.

use crate::ast::MatchType;

/// Match-type comparison. All comparisons are case-insensitive.
pub(crate) fn match_string(mt: MatchType, haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    match mt {
        MatchType::Is => h.eq_ignore_ascii_case(n),
        MatchType::Contains => contains_ci(h, n),
        MatchType::Matches => glob_match_ci(h, n),
    }
}

/// Case-insensitive substring search. Uses `memchr2` on the
/// lowercase + uppercase variant of the needle's first byte to
/// jump from candidate to candidate without re-scanning every
/// position.
fn contains_ci(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    find_first_ci(haystack, needle).is_some()
}

/// Tiny glob matcher: `*` matches any sequence, `?` matches one
/// byte. ASCII-only — sufficient for RFC 5228 `:matches`. Only
/// referenced by the test module; the live path goes through
/// `match_string` → `glob_match_ci` directly.
#[cfg(test)]
fn glob_match(haystack: &str, pattern: &str) -> bool {
    glob_match_ci(haystack.as_bytes(), pattern.as_bytes())
}

/// Case-insensitive glob match. Splits the pattern on `*` and
/// searches each literal chunk in order; the recursive
/// byte-by-byte path is replaced by `memchr2`-anchored substring
/// search inside each chunk.
fn glob_match_ci(haystack: &[u8], pattern: &[u8]) -> bool {
    if pattern.is_empty() {
        return haystack.is_empty();
    }

    let is_anchored_start = pattern[0] != b'*';
    let is_anchored_end = pattern[pattern.len() - 1] != b'*';

    // Split the pattern on `*`, collapsing runs of `*` (`**` ≡ `*`).
    let mut chunks: Vec<&[u8]> = Vec::with_capacity(4);
    let mut start = 0;
    let mut i = 0;
    while i < pattern.len() {
        if pattern[i] == b'*' {
            if start < i {
                chunks.push(&pattern[start..i]);
            }
            while i < pattern.len() && pattern[i] == b'*' {
                i += 1;
            }
            start = i;
        } else {
            i += 1;
        }
    }
    if start < pattern.len() {
        chunks.push(&pattern[start..]);
    }

    if chunks.is_empty() {
        // Pattern was entirely `*`s.
        return true;
    }

    let mut cursor = 0usize;
    let last = chunks.len() - 1;
    for (idx, chunk) in chunks.iter().enumerate() {
        let must_anchor_start = idx == 0 && is_anchored_start;
        let must_anchor_end = idx == last && is_anchored_end;

        if must_anchor_start {
            if cursor + chunk.len() > haystack.len() {
                return false;
            }
            if !match_chunk_ci(&haystack[cursor..cursor + chunk.len()], chunk) {
                return false;
            }
            cursor += chunk.len();
        } else if must_anchor_end {
            if chunk.len() > haystack.len().saturating_sub(cursor) {
                return false;
            }
            let tail_start = haystack.len() - chunk.len();
            if tail_start < cursor {
                return false;
            }
            if !match_chunk_ci(&haystack[tail_start..], chunk) {
                return false;
            }
            cursor = haystack.len();
        } else {
            match find_chunk_ci(&haystack[cursor..], chunk) {
                Some(rel) => cursor += rel + chunk.len(),
                None => return false,
            }
        }
    }

    // If the pattern's tail wasn't anchored we already covered everything
    // we needed; if it WAS anchored, the must_anchor_end branch above
    // already pushed cursor to haystack.len().
    true
}

/// Equal-length comparison treating `?` in `chunk` as a wildcard
/// and other bytes as case-insensitive literals.
fn match_chunk_ci(haystack: &[u8], chunk: &[u8]) -> bool {
    if haystack.len() != chunk.len() {
        return false;
    }
    for (h, p) in haystack.iter().zip(chunk.iter()) {
        if *p == b'?' {
            continue;
        }
        if !h.eq_ignore_ascii_case(p) {
            return false;
        }
    }
    true
}

/// Find the leftmost case-insensitive position where `chunk`
/// matches inside `haystack`. `?` in chunk is a one-byte wildcard.
fn find_chunk_ci(haystack: &[u8], chunk: &[u8]) -> Option<usize> {
    if chunk.is_empty() {
        return Some(0);
    }
    if chunk.len() > haystack.len() {
        return None;
    }
    if !chunk.contains(&b'?') {
        return find_first_ci(haystack, chunk);
    }
    // Anchor on the first non-`?` byte (if any) via memchr2 to
    // skip dead candidate starts.
    let anchor_off = match chunk.iter().position(|&c| c != b'?') {
        Some(o) => o,
        None => return Some(0), // all `?` — matches every offset
    };
    let anchor = chunk[anchor_off];
    let lo = anchor.to_ascii_lowercase();
    let up = anchor.to_ascii_uppercase();
    let mut start = 0;
    while start + chunk.len() <= haystack.len() {
        let search_from = start + anchor_off;
        if search_from >= haystack.len() {
            return None;
        }
        let rel = memchr::memchr2(lo, up, &haystack[search_from..])?;
        let abs_anchor = search_from + rel;
        if abs_anchor < anchor_off {
            return None;
        }
        let candidate = abs_anchor - anchor_off;
        if candidate + chunk.len() > haystack.len() {
            return None;
        }
        if match_chunk_ci(&haystack[candidate..candidate + chunk.len()], chunk) {
            return Some(candidate);
        }
        start = candidate + 1;
    }
    None
}

/// Case-insensitive byte search for `needle` (no `?` wildcard) in
/// `haystack`.
fn find_first_ci(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    let first_lo = needle[0].to_ascii_lowercase();
    let first_up = needle[0].to_ascii_uppercase();
    let mut start = 0;
    while start + needle.len() <= haystack.len() {
        let rel = memchr::memchr2(first_lo, first_up, &haystack[start..])?;
        let pos = start + rel;
        if pos + needle.len() > haystack.len() {
            return None;
        }
        if haystack[pos..pos + needle.len()].eq_ignore_ascii_case(needle) {
            return Some(pos);
        }
        start = pos + 1;
    }
    None
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
    fn contains_empty_needle() {
        assert!(match_string(MatchType::Contains, "anything", ""));
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

    #[test]
    fn glob_case_insensitive() {
        assert!(glob_match("Hello World", "*WORLD"));
        assert!(glob_match("foo.bar@example.com", "*@EXAMPLE.com"));
    }

    #[test]
    fn glob_multi_star() {
        assert!(glob_match("hello kind world", "hello*kind*world"));
        assert!(!glob_match("hello world", "hello*kind*world"));
        assert!(glob_match("aXbYc", "a*b*c"));
        // Collapsed `**` is identical to `*`.
        assert!(glob_match("abcdef", "a**f"));
    }

    #[test]
    fn glob_question_inside_star_chunk() {
        assert!(glob_match("abcXdef", "*c?d*"));
        assert!(!glob_match("abcdef", "*c?d?g"));
    }

    #[test]
    fn glob_anchored_both_sides_with_mid_star() {
        assert!(glob_match("foobar", "foo*bar"));
        assert!(!glob_match("foobar", "baz*bar"));
    }
}

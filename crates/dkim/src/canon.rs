//! Canonicalization (RFC 6376 §3.4).
//!
//! Two variants for each of {header, body}: `simple` (preserve as-is
//! with minor tweaks) and `relaxed` (normalize whitespace + lowercase
//! header names).

use crate::header::Canon;

/// Canonicalize the body bytes per `body_canon`.
///
/// `simple` (RFC 6376 §3.4.3):
/// - All trailing empty lines (CRLF runs at end) collapse to one CRLF
/// - If the body is empty, return `"\r\n"`
///
/// `relaxed` (§3.4.4):
/// - Within each line, replace WSP runs with a single SP
/// - Strip trailing WSP from each line
/// - Then apply the `simple` trailing-CRLF rule
///
/// Optional `l=` length limit: after canonicalization, truncate to
/// the first `l` bytes.
pub fn canonicalize_body(input: &[u8], canon: Canon, length: Option<u64>) -> Vec<u8> {
    let stage1: Vec<u8> = match canon {
        Canon::Simple => input.to_vec(),
        Canon::Relaxed => relax_body(input),
    };
    let stage2 = collapse_trailing_crlf(&stage1);
    match length {
        Some(l) if (l as usize) < stage2.len() => stage2[..l as usize].to_vec(),
        _ => stage2,
    }
}

/// Canonicalize a single header field per `header_canon`.
///
/// `name` is the header field name without trailing `:`. `value` is
/// everything after the first `:` (the header-field value, possibly
/// folded across multiple lines).
///
/// `simple` (RFC 6376 §3.4.1): emit `Name: value\r\n` with no
/// transformation other than ensuring CRLF terminator.
///
/// `relaxed` (§3.4.2):
/// - Lowercase the name
/// - Unfold WSP+CRLF sequences in the value
/// - Replace WSP runs (including across the unfolded value) with one SP
/// - Strip trailing WSP from the value
/// - Strip WSP between the name and the `:`
/// - One SP after the `:`
/// - Terminate with CRLF
pub fn canonicalize_header(name: &str, value: &str, canon: Canon) -> Vec<u8> {
    match canon {
        Canon::Simple => {
            // simple: emit Name: <value>\r\n verbatim, but value's
            // existing CRLF is preserved internally (no unfold), only
            // the final CRLF is enforced.
            let mut out = Vec::with_capacity(name.len() + 2 + value.len() + 2);
            out.extend_from_slice(name.as_bytes());
            out.push(b':');
            out.extend_from_slice(value.as_bytes());
            // Ensure trailing CRLF
            if !out.ends_with(b"\r\n") {
                out.extend_from_slice(b"\r\n");
            }
            out
        }
        Canon::Relaxed => {
            let lowered = name.to_ascii_lowercase();
            let unfolded = unfold_value(value);
            let normalized = normalize_wsp(&unfolded);
            let trimmed = normalized.trim_end_matches([' ', '\t']);
            // RFC 6376 §3.4.2: strip leading WSP, then output as
            // "name:value\r\n".
            let trimmed = trimmed.trim_start_matches([' ', '\t']);
            let mut out = Vec::with_capacity(lowered.len() + 1 + trimmed.len() + 2);
            out.extend_from_slice(lowered.as_bytes());
            out.push(b':');
            out.extend_from_slice(trimmed.as_bytes());
            out.extend_from_slice(b"\r\n");
            out
        }
    }
}

/// Apply relaxed body canon transforms PER LINE: WSP run → single SP,
/// strip trailing WSP. Then trailing-CRLF collapse is applied
/// separately by [`canonicalize_body`].
///
/// memchr-anchored: scans for `\n` to delimit lines and for `' '|'\t'`
/// to find the next WSP run inside a line, copying the run-between
/// clean spans in bulk via `extend_from_slice` (memcpy). Replaces the
/// previous nested per-byte loop + `Vec<&[u8]>` allocation that
/// `split_lines_keep_crlf` did upfront.
fn relax_body(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut pos = 0;
    while pos < input.len() {
        // Find the line end (LF). content_end is the byte after the
        // last content byte (excludes any \r\n or \n terminator).
        let lf_opt = memchr::memchr(b'\n', &input[pos..]).map(|o| pos + o);
        let (content_end, after_line, had_crlf) = match lf_opt {
            Some(lf) => {
                let ce = if lf > pos && input[lf - 1] == b'\r' {
                    lf - 1
                } else {
                    lf
                };
                (ce, lf + 1, true)
            }
            None => (input.len(), input.len(), false),
        };

        // Collapse WSP runs to a single space. Walk the content using
        // memchr2 to anchor on the next WSP, copying the clean run
        // before it as one memcpy.
        let line_start_in_out = out.len();
        let mut cur = pos;
        while cur < content_end {
            match memchr::memchr2(b' ', b'\t', &input[cur..content_end]) {
                None => {
                    out.extend_from_slice(&input[cur..content_end]);
                    break;
                }
                Some(off) => {
                    let wsp_pos = cur + off;
                    out.extend_from_slice(&input[cur..wsp_pos]);
                    out.push(b' ');
                    // Skip the WSP run.
                    let mut next = wsp_pos + 1;
                    while next < content_end && matches!(input[next], b' ' | b'\t') {
                        next += 1;
                    }
                    cur = next;
                }
            }
        }

        // Strip trailing WSP from this line's region in `out`.
        while out.len() > line_start_in_out && matches!(out[out.len() - 1], b' ' | b'\t') {
            out.pop();
        }

        // Emit CRLF if the input line had any line terminator (\r\n
        // or bare \n).
        if had_crlf {
            out.extend_from_slice(b"\r\n");
        }
        pos = after_line;
    }
    out
}

/// Drop trailing empty lines, ensure exactly one terminating CRLF.
/// Empty body → "\r\n".
fn collapse_trailing_crlf(input: &[u8]) -> Vec<u8> {
    if input.is_empty() {
        return b"\r\n".to_vec();
    }
    let mut end = input.len();
    // Walk back over trailing CRLF / LF sequences
    while end >= 2 && &input[end - 2..end] == b"\r\n" {
        end -= 2;
    }
    while end >= 1 && input[end - 1] == b'\n' {
        end -= 1;
    }
    let mut out = input[..end].to_vec();
    out.extend_from_slice(b"\r\n");
    out
}

/// Unfold: CRLF (or LF) followed by WSP → single SP (the WSP itself
/// is collapsed away). Used by the relaxed header canon.
fn unfold_value(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            // CRLF + WSP → drop the CRLF (the WSP stays; then WSP-norm
            // collapses it to one SP).
            if i + 2 < bytes.len() && matches!(bytes[i + 2], b' ' | b'\t') {
                i += 2; // skip CRLF
                continue;
            }
            // CRLF without following WSP shouldn't happen inside a
            // header value, but we handle it gracefully — drop both.
            i += 2;
            continue;
        }
        if bytes[i] == b'\n' && i + 1 < bytes.len() && matches!(bytes[i + 1], b' ' | b'\t') {
            i += 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Collapse any run of WSP (SP / HTAB) into one SP. Internal use of
/// the relaxed header canon.
fn normalize_wsp(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_wsp = false;
    for c in input.chars() {
        if c == ' ' || c == '\t' {
            if !prev_wsp {
                out.push(' ');
                prev_wsp = true;
            }
        } else {
            out.push(c);
            prev_wsp = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_simple_empty_returns_crlf() {
        let r = canonicalize_body(b"", Canon::Simple, None);
        assert_eq!(r, b"\r\n");
    }

    #[test]
    fn body_simple_trailing_empty_lines_collapse() {
        let r = canonicalize_body(b"hello\r\n\r\n\r\n", Canon::Simple, None);
        assert_eq!(r, b"hello\r\n");
    }

    #[test]
    fn body_simple_one_crlf_preserved() {
        let r = canonicalize_body(b"hello\r\n", Canon::Simple, None);
        assert_eq!(r, b"hello\r\n");
    }

    #[test]
    fn body_simple_no_trailing_crlf_added() {
        let r = canonicalize_body(b"hello", Canon::Simple, None);
        assert_eq!(r, b"hello\r\n");
    }

    #[test]
    fn body_relaxed_collapses_wsp_within_line() {
        let r = canonicalize_body(b"a  b   c\r\n", Canon::Relaxed, None);
        assert_eq!(r, b"a b c\r\n");
    }

    #[test]
    fn body_relaxed_strips_trailing_wsp() {
        let r = canonicalize_body(b"hello   \r\n", Canon::Relaxed, None);
        assert_eq!(r, b"hello\r\n");
    }

    #[test]
    fn body_relaxed_then_trailing_collapse() {
        let r = canonicalize_body(b"foo\t bar  \r\n\r\n\r\n", Canon::Relaxed, None);
        assert_eq!(r, b"foo bar\r\n");
    }

    #[test]
    fn body_length_limit_truncates_after_canon() {
        // Canon first → "hello\r\n", then truncate to 3 → "hel"
        let r = canonicalize_body(b"hello\r\n", Canon::Simple, Some(3));
        assert_eq!(r, b"hel");
    }

    #[test]
    fn header_simple_emits_verbatim_with_crlf() {
        let r = canonicalize_header("From", " alice@example.com", Canon::Simple);
        assert_eq!(r, b"From: alice@example.com\r\n");
    }

    #[test]
    fn header_simple_preserves_trailing_wsp() {
        // simple preserves the value verbatim (modulo ensuring CRLF terminator)
        let r = canonicalize_header("Subject", " test  ", Canon::Simple);
        assert_eq!(r, b"Subject: test  \r\n");
    }

    #[test]
    fn header_relaxed_lowercases_name() {
        let r = canonicalize_header("From", " alice@example.com", Canon::Relaxed);
        assert_eq!(r, b"from:alice@example.com\r\n");
    }

    #[test]
    fn header_relaxed_strips_leading_wsp() {
        let r = canonicalize_header("Subject", "    hello", Canon::Relaxed);
        assert_eq!(r, b"subject:hello\r\n");
    }

    #[test]
    fn header_relaxed_collapses_internal_wsp() {
        let r = canonicalize_header("Subject", " hello   world  ", Canon::Relaxed);
        assert_eq!(r, b"subject:hello world\r\n");
    }

    #[test]
    fn header_relaxed_unfolds_continuation() {
        // Value with folded continuation:  "first\r\n  second"
        let r = canonicalize_header("X-Long", " first\r\n  second  third", Canon::Relaxed);
        // After unfold + WSP norm: "first second third"
        assert_eq!(r, b"x-long:first second third\r\n");
    }

    #[test]
    fn header_relaxed_handles_tabs() {
        let r = canonicalize_header("X", "\thello\tworld", Canon::Relaxed);
        assert_eq!(r, b"x:hello world\r\n");
    }
}

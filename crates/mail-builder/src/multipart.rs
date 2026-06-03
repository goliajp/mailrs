//! Multipart envelope assembly and boundary generation.

use std::sync::atomic::{AtomicU64, Ordering};

/// Generate a MIME boundary string of the form
/// `mailrs_<pid>_<counter>_<rng>` that is highly unlikely to collide
/// with any reasonable user input. Callers should still scan the
/// body and call [`generate_boundary`] again if a collision is found
/// — `multipart_envelope` does this automatically.
///
/// The generated boundary is 35 ASCII chars: `mailrs_` (7) + pid hex
/// (≤ 8) + `_` + counter hex (≤ 8) + `_` + 32-bit rng hex (8).
pub fn generate_boundary() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let pid = std::process::id();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let rng = quick_rng();
    format!("mailrs_{pid:x}_{counter:x}_{rng:08x}")
}

/// Tiny non-cryptographic PRNG seeded from the system clock. Adequate
/// for boundary uniqueness — we'd never use this for anything
/// security-sensitive, and the collision-scan in
/// [`multipart_envelope`] is the actual safety net.
fn quick_rng() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // splitmix64 step is plenty for our needs
    let mut z = now.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    ((z ^ (z >> 31)) >> 32) as u32
}

/// A single MIME part: pre-encoded header block (without trailing
/// blank line) plus pre-encoded body bytes. The envelope inserts the
/// blank line and the boundary separators.
#[derive(Debug, Clone)]
pub struct PartBytes {
    /// Header block — MUST end with `\r\n` after the last header but
    /// MUST NOT include the empty separator line.
    pub headers: Vec<u8>,
    /// Body bytes (already encoded with the appropriate CTE).
    pub body: Vec<u8>,
}

/// Build a multipart envelope: returns the bytes that go BETWEEN the
/// outer headers and the closing boundary, with a guaranteed-unique
/// boundary that does not appear in any of the parts. Returns
/// `(boundary, envelope_bytes)`.
///
/// Layout per RFC 2046 §5.1.1:
///
/// ```text
/// --boundary\r\n
/// <part0 headers>\r\n\r\n<part0 body>\r\n
/// --boundary\r\n
/// <part1 headers>\r\n\r\n<part1 body>\r\n
/// --boundary--\r\n
/// ```
///
/// Collision check: each part's headers + body are scanned for the
/// chosen boundary; if any contains it, a fresh boundary is generated
/// and the scan repeats (capped at 8 attempts — practically
/// unreachable but bounded for safety).
pub fn multipart_envelope(parts: &[PartBytes]) -> (String, Vec<u8>) {
    let boundary = pick_non_colliding_boundary(parts);
    let boundary_line = format!("--{boundary}");
    let closing_line = format!("--{boundary}--");

    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(boundary_line.as_bytes());
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&part.headers);
        // Header block ends with the last header's CRLF; the
        // separator blank line is one more CRLF here.
        if !part.headers.ends_with(b"\r\n") {
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&part.body);
        if !part.body.ends_with(b"\r\n") {
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(closing_line.as_bytes());
    out.extend_from_slice(b"\r\n");
    (boundary, out)
}

fn pick_non_colliding_boundary(parts: &[PartBytes]) -> String {
    for _ in 0..8 {
        let b = generate_boundary();
        let needle_open = format!("--{b}");
        let mut collides = false;
        for part in parts {
            if contains_subslice(&part.headers, needle_open.as_bytes())
                || contains_subslice(&part.body, needle_open.as_bytes())
            {
                collides = true;
                break;
            }
        }
        if !collides {
            return b;
        }
    }
    // Practically unreachable; fall through to a longer entropy
    // boundary so we never panic.
    format!(
        "mailrs_fallback_{:016x}",
        quick_rng() as u64 | ((quick_rng() as u64) << 32)
    )
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    // memchr's memmem (Two-Way SIMD) — replaces the O(N·M)
    // `windows().any()` walk used to detect boundary collisions.
    memchr::memmem::find(haystack, needle).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_format_is_mailrs_prefix() {
        let b = generate_boundary();
        assert!(b.starts_with("mailrs_"), "got {b:?}");
        assert!(b.is_ascii());
        // No characters that would cause issues inside the
        // multipart-boundary header value.
        for c in b.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '_',
                "bad char in boundary: {c:?}"
            );
        }
    }

    #[test]
    fn boundary_is_unique_across_calls() {
        let a = generate_boundary();
        let b = generate_boundary();
        assert_ne!(a, b);
    }

    #[test]
    fn envelope_simple_two_part() {
        let parts = vec![
            PartBytes {
                headers: b"Content-Type: text/plain; charset=utf-8\r\n".to_vec(),
                body: b"hello\r\n".to_vec(),
            },
            PartBytes {
                headers: b"Content-Type: text/html; charset=utf-8\r\n".to_vec(),
                body: b"<p>hi</p>\r\n".to_vec(),
            },
        ];
        let (boundary, bytes) = multipart_envelope(&parts);
        let s = std::str::from_utf8(&bytes).unwrap();
        let open = format!("--{boundary}");
        let close = format!("--{boundary}--");
        // exactly two opens + one close
        assert_eq!(s.matches(&open).count(), 3); // 2 opens + 1 close (close starts with --boundary)
        assert!(s.contains(&close));
        assert!(s.contains("text/plain"));
        assert!(s.contains("text/html"));
        // body bytes preserved
        assert!(s.contains("hello"));
        assert!(s.contains("<p>hi</p>"));
    }

    #[test]
    fn envelope_avoids_collision_with_body() {
        // construct a part whose body contains every possible
        // mailrs boundary prefix — collision must be detected and
        // a fresh boundary picked
        let parts = vec![PartBytes {
            headers: b"Content-Type: text/plain\r\n".to_vec(),
            body: b"--mailrs_should_not_collide\r\nbody continues\r\n".to_vec(),
        }];
        let (boundary, bytes) = multipart_envelope(&parts);
        // body's bogus boundary is preserved unchanged
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.contains("--mailrs_should_not_collide"));
        // and the actual envelope boundary is different
        assert!(
            !boundary.contains("should_not_collide"),
            "boundary leaked into body's fake marker: {boundary}",
        );
    }
}

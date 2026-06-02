//! DATA-stage helpers: remove SMTP dot-stuffing from message bodies.
//!
//! RFC 5321 § 4.5.2 requires that any line in the DATA payload starting with
//! `.` be prefixed with an extra `.` on the wire, and that a lone `.` on its
//! own line terminate the message. These helpers reverse that transform.

/// Remove dot-stuffing from a single DATA line.
///
/// Returns `Some(unstuffed_line)` for normal lines, or `None` if the line is
/// the terminator `".\r\n"` — caller should stop reading at that point.
pub fn unstuff_line(line: &[u8]) -> Option<&[u8]> {
    if line == b".\r\n" {
        return None;
    }
    if line.starts_with(b"..") {
        Some(&line[1..])
    } else {
        Some(line)
    }
}

/// Process a complete DATA payload (CRLF-terminated lines, ending with
/// `".\r\n"`). Returns the message body with dot-stuffing removed and the
/// `.` terminator stripped.
pub fn unstuff_data(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut start = 0;
    while start < data.len() {
        // memchr is SIMD-vectorised; the previous `.iter().position`
        // scan was the per-line cost in the inbound DATA hot path
        // (every message body line is scanned).
        let end = match memchr::memchr(b'\n', &data[start..]) {
            Some(pos) => start + pos + 1,
            None => data.len(),
        };
        let line = &data[start..end];
        match unstuff_line(line) {
            Some(unstuffed) => result.extend_from_slice(unstuffed),
            None => break,
        }
        start = end;
    }
    result
}

#[cfg(test)]
mod tests;

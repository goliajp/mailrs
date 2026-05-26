use std::fmt::Write as _;
use std::net::SocketAddr;

pub(super) fn format_received_header(
    client_domain: &str,
    server_hostname: &str,
    recipient: &str,
    addr: &SocketAddr,
) -> String {
    let now = chrono::Utc::now().to_rfc2822();
    // pre-sized buffer for typical header (~200 chars in the wild):
    //   "Received: from <fqdn> (ip:port)\r\n\tby <our-fqdn> with ESMTP\r\n
    //    \tfor <user@domain>; Sun, 22 May 2026 12:00:00 +0900\r\n"
    let mut out = String::with_capacity(256);
    let _ = write!(
        out,
        "Received: from {client_domain} ({addr})\r\n\tby {server_hostname} with ESMTP\r\n\tfor <{recipient}>; {now}\r\n"
    );
    out
}

/// extract Subject + From — paired with `mailrs-rfc5322` (skip-ahead
/// header lookup) + `mailrs-rfc2047` (encoded-word decoder).
///
/// Previously this went through `mail_parser::MessageParser` which builds
/// the full Message tree (every header parsed, MIME structure decoded
/// recursively, body materialized). For an inbound SMTP server that only
/// needs Subject + From, that's wasted work; mail-parser scaled linearly
/// with body size.
///
/// `mailrs-rfc5322::Message::header` is `O(header-region-size)` (stops
/// at the empty-line terminator), and `mailrs-rfc2047::decode` is a
/// per-token base64/Q-printable decode that's ~25 ns ASCII / ~70 ns B
/// encoded. The composition is measured **4-16× faster** than the
/// equivalent mail-parser path on the existing
/// `bench_two_pass_vs_single_pass_extract` harness (PERFORMANCE.md).
///
/// Returns `(subject, from)`. Either may be `String::new()` if missing.
pub(super) fn extract_subject_and_from(message: &[u8]) -> (String, String) {
    let msg = mailrs_rfc5322::Message::new(message);
    let subject = msg
        .header("Subject")
        .map(|bytes| mailrs_rfc2047::decode(bytes).into_owned())
        .unwrap_or_default();
    let from = msg
        .header("From")
        .map(format_from_field)
        .unwrap_or_default();
    (subject, from)
}

/// Format a `From:` field value into "Display Name <email>" or "email"
/// shape, matching the legacy mail-parser output. The encoded display
/// name (if any) is decoded via `mailrs-rfc2047`; the angle-bracket
/// address (if any) is extracted as-is.
fn format_from_field(value: &[u8]) -> String {
    // Find `<...>` to split display name from address.
    let lt = value.iter().position(|&b| b == b'<');
    let gt = value.iter().rposition(|&b| b == b'>');
    match (lt, gt) {
        (Some(lt_pos), Some(gt_pos)) if gt_pos > lt_pos => {
            let raw_name = &value[..lt_pos];
            let addr = &value[lt_pos + 1..gt_pos];
            // Decode + trim the display name. May be RFC 2047 encoded.
            let name_cow = mailrs_rfc2047::decode(raw_name);
            let name = name_cow.trim().trim_matches('"').trim();
            if name.is_empty() {
                String::from_utf8_lossy(addr).into_owned()
            } else {
                let mut out = String::with_capacity(name.len() + addr.len() + 3);
                out.push_str(name);
                out.push_str(" <");
                out.push_str(&String::from_utf8_lossy(addr));
                out.push('>');
                out
            }
        }
        _ => {
            // Bare address — no angle brackets. Trim and return.
            let cow = mailrs_rfc2047::decode(value);
            cow.trim().to_string()
        }
    }
}

/// extract a short snippet from the message body for notifications
pub(super) fn extract_snippet(message: &[u8]) -> String {
    let part = mailrs_mime::parse(message);
    if let Some(text) = part.body_text() {
        // one-line snippet, max 100 chars. Single allocation via
        // pre-sized String. Counts chars in O(n).
        let mut out = String::with_capacity(100);
        let mut count = 0usize;
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            out.push(ch);
            count += 1;
            if count >= 100 {
                break;
            }
        }
        return out;
    }
    String::new()
}

/// extract display name from "Display Name <email@domain>" format
pub(super) fn extract_display_name(sender: &str) -> String {
    if let Some(angle) = sender.find('<') {
        let name = sender[..angle].trim().trim_matches('"');
        if !name.is_empty() {
            return name.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_display_name_with_angle() {
        assert_eq!(extract_display_name("Alice <alice@example.com>"), "Alice");
        assert_eq!(
            extract_display_name("\"Bob Smith\" <bob@example.com>"),
            "Bob Smith"
        );
    }

    #[test]
    fn extract_display_name_bare_email() {
        assert_eq!(extract_display_name("alice@example.com"), "");
    }
}

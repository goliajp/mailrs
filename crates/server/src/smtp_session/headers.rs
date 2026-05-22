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

/// extract a header value from raw message bytes (with RFC 2047 decoding)
pub(super) fn extract_header(message: &[u8], name: &str) -> String {
    // use mail-parser for proper RFC 2047 encoded-word decoding
    if let Some(msg) = mail_parser::MessageParser::default().parse(message) {
        match name.to_lowercase().as_str() {
            "subject" => {
                if let Some(s) = msg.subject() {
                    return s.to_string();
                }
            }
            "from" => {
                if let Some(addr) = msg.from().and_then(|a| a.first()) {
                    return match addr.name() {
                        Some(name) => format!("{} <{}>", name, addr.address().unwrap_or("")),
                        None => addr.address().unwrap_or("").to_string(),
                    };
                }
            }
            _ => {}
        }
    }
    // fallback: naive line scan, no full UTF-8 transcode.
    // bytes pre-scan: most headers are ASCII; we only String-build the match.
    let name_lower = name.as_bytes().to_ascii_lowercase();
    let mut start = 0;
    while start < message.len() {
        // find end of line (\r\n or \n)
        let lf = message[start..].iter().position(|&b| b == b'\n');
        let end = match lf {
            Some(i) => start + i,
            None => message.len(),
        };
        let mut line_end = end;
        if line_end > start && message[line_end - 1] == b'\r' {
            line_end -= 1;
        }
        let line = &message[start..line_end];
        // blank line ends header block
        if line.is_empty() {
            break;
        }
        // header line must have colon; compare name prefix case-insensitively
        if let Some(colon) = line.iter().position(|&b| b == b':')
            && colon == name_lower.len()
            && line[..colon]
                .iter()
                .zip(name_lower.iter())
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            let value = &line[colon + 1..];
            // trim leading whitespace, return lossy-decoded owned string
            let trimmed = trim_ascii_left(value);
            return String::from_utf8_lossy(trimmed).trim().to_string();
        }
        start = end + 1;
    }
    String::new()
}

#[inline]
fn trim_ascii_left(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
        i += 1;
    }
    &s[i..]
}

/// extract a short snippet from the message body for notifications
pub(super) fn extract_snippet(message: &[u8]) -> String {
    if let Some(msg) = mail_parser::MessageParser::default().parse(message)
        && let Some(text) = msg.body_text(0)
    {
        // one-line snippet, max 100 chars. Single allocation via
        // pre-sized String (was: chars().take(100).collect → second alloc
        // via lines().next().to_string). Counts chars in O(n), not O(n²).
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
        assert_eq!(extract_display_name("\"Bob Smith\" <bob@example.com>"), "Bob Smith");
    }

    #[test]
    fn extract_display_name_bare_email() {
        assert_eq!(extract_display_name("alice@example.com"), "");
    }
}

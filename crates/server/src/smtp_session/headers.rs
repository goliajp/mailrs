use std::net::SocketAddr;

pub(super) fn format_received_header(
    client_domain: &str,
    server_hostname: &str,
    recipient: &str,
    addr: &SocketAddr,
) -> String {
    let now = chrono::Utc::now().to_rfc2822();
    format!(
        "Received: from {client_domain} ({addr})\r\n\tby {server_hostname} with ESMTP\r\n\tfor <{recipient}>; {now}\r\n"
    )
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
    // fallback: naive line extraction
    let text = String::from_utf8_lossy(message);
    let prefix = format!("{name}:");
    for line in text.lines() {
        if line.len() > prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            return line[prefix.len()..].trim().to_string();
        }
        if line.is_empty() {
            break;
        }
    }
    String::new()
}

/// extract a short snippet from the message body for notifications
pub(super) fn extract_snippet(message: &[u8]) -> String {
    if let Some(msg) = mail_parser::MessageParser::default().parse(message)
        && let Some(text) = msg.body_text(0) {
            let s: String = text.chars().take(100).collect();
            return s.lines().next().unwrap_or("").to_string();
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

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
    let client = trace_name(client_domain);
    let _ = write!(
        out,
        "Received: from {client} ({addr})\r\n\tby {server_hostname} with ESMTP\r\n\tfor <{recipient}>; {now}\r\n"
    );
    out
}

/// The client's EHLO name as it may stand in the trace. The client
/// chooses it, and the first `(` of this line is where the peer address
/// is read back from, so only what a host name or address literal is made
/// of is kept.
fn trace_name(ehlo: &str) -> String {
    let kept: String = ehlo
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':'))
        .take(255)
        .collect();
    if kept.is_empty() {
        return "unknown".to_string();
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trace_names_the_client_and_reads_back_its_address() {
        let addr: SocketAddr = "203.0.113.7:4242".parse().unwrap();
        let line = format_received_header("mx.example.org", "mail.golia.ai", "a@golia.jp", &addr);
        assert!(
            line.starts_with("Received: from mx.example.org (203.0.113.7:4242)"),
            "{line}"
        );
        assert_eq!(
            mailrs_inbound::identity::first_received_peer_ip(line.as_bytes()),
            Some(addr.ip())
        );
    }

    #[test]
    fn a_client_cannot_write_its_own_address_into_the_trace() {
        let addr: SocketAddr = "203.0.113.7:4242".parse().unwrap();
        let line =
            format_received_header("evil (10.0.0.1:25)", "mail.golia.ai", "a@golia.jp", &addr);
        assert_eq!(
            mailrs_inbound::identity::first_received_peer_ip(line.as_bytes()),
            Some(addr.ip())
        );
        let empty = format_received_header("", "mail.golia.ai", "a@golia.jp", &addr);
        assert!(
            empty.starts_with("Received: from unknown (203.0.113.7:4242)"),
            "{empty}"
        );
    }
}

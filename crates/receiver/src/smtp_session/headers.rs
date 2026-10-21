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

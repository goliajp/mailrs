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

/// extract Subject + From in a single mail-parser pass — both are read
/// per inbound message, and `mail_parser::MessageParser` does a non-trivial
/// pre-scan + tree build. Calling [`extract_header`] twice (once for
/// "Subject", once for "From") parses the message twice; this helper
/// hands back both values for one parse.
///
/// Returns `(subject, from)`. Either may be `String::new()` if missing.
pub(super) fn extract_subject_and_from(message: &[u8]) -> (String, String) {
    if let Some(msg) = mail_parser::MessageParser::default().parse(message) {
        let subject = msg.subject().map(|s| s.to_string()).unwrap_or_default();
        let from = msg
            .from()
            .and_then(|a| a.first())
            .map(|addr| match addr.name() {
                Some(n) => format!("{} <{}>", n, addr.address().unwrap_or("")),
                None => addr.address().unwrap_or("").to_string(),
            })
            .unwrap_or_default();
        return (subject, from);
    }
    (String::new(), String::new())
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
    use std::time::Instant;

    #[test]
    fn extract_display_name_with_angle() {
        assert_eq!(extract_display_name("Alice <alice@example.com>"), "Alice");
        assert_eq!(extract_display_name("\"Bob Smith\" <bob@example.com>"), "Bob Smith");
    }

    #[test]
    fn extract_display_name_bare_email() {
        assert_eq!(extract_display_name("alice@example.com"), "");
    }

    // Old two-call implementation, kept solely for the
    // bench_two_pass_vs_single_pass test below to measure the actual
    // before/after delta of commit 6e7a721.
    fn extract_header_legacy(message: &[u8], name: &str) -> String {
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
        String::new()
    }

    fn sample_message(body_size: usize) -> Vec<u8> {
        let mut msg = Vec::with_capacity(2048 + body_size);
        msg.extend_from_slice(
            b"Return-Path: <alice@example.com>\r\n\
              Received: from mta.example.com (mta.example.com [203.0.113.42])\r\n\
                  \tby mx.golia.jp with ESMTP id 12345; Sun, 22 May 2026 10:00:00 +0900\r\n\
              From: \"Alice Liddell\" <alice@example.com>\r\n\
              To: <bob@golia.jp>\r\n\
              Subject: Important: Q4 numbers for review\r\n\
              Date: Sun, 22 May 2026 09:55:00 +0900\r\n\
              Message-ID: <abc-123@example.com>\r\n\
              MIME-Version: 1.0\r\n\
              Content-Type: text/plain; charset=utf-8\r\n\
              Content-Transfer-Encoding: 7bit\r\n\r\n",
        );
        // body
        for _ in 0..(body_size / 80) {
            msg.extend_from_slice(b"This is a typical inbound message body line, ASCII text only.\r\n");
        }
        msg
    }

    fn median_iter<F: FnMut()>(iters: usize, mut op: F) -> u128 {
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let start = Instant::now();
            op();
            samples.push(start.elapsed().as_nanos());
        }
        samples.sort();
        samples[iters / 2]
    }

    /// Honest before/after measurement of commit 6e7a721's claim. The
    /// commit said "Estimated 50-80 µs saved per inbound message" —
    /// this test verifies the real number rather than relying on the
    /// estimate. Output via `cargo test -- --nocapture` + the
    /// MAILRS_BENCH=1 env gate, so normal CI doesn't print anything.
    #[test]
    fn bench_two_pass_vs_single_pass_extract() {
        if std::env::var("MAILRS_BENCH").is_err() {
            return;
        }
        for &sz in &[1_000usize, 5_000, 20_000] {
            let msg = sample_message(sz);
            let two_pass = median_iter(200, || {
                let s = extract_header_legacy(&msg, "Subject");
                let f = extract_header_legacy(&msg, "From");
                std::hint::black_box((s, f));
            });
            let single_pass = median_iter(200, || {
                let (s, f) = extract_subject_and_from(&msg);
                std::hint::black_box((s, f));
            });
            let saved = two_pass.saturating_sub(single_pass);
            let pct = (saved as f64 / two_pass as f64) * 100.0;
            eprintln!(
                "  msg_size={sz:>6}  two_pass={two_pass:>6}ns  single_pass={single_pass:>6}ns  saved={saved:>6}ns ({pct:.1}%)"
            );
        }
    }
}

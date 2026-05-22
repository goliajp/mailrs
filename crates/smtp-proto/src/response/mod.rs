//! SMTP response codes, enhanced status codes, and well-known reply
//! constructors.

use std::fmt::Write;

/// An SMTP reply: a 3-digit status code, an optional enhanced status code
/// (RFC 3463), and a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Three-digit SMTP reply code (e.g. `250`, `554`, `421`).
    pub code: u16,
    /// Optional enhanced status code (`class.subject.detail`).
    pub enhanced: Option<EnhancedCode>,
    /// Human-readable reply text (without the trailing CRLF).
    pub message: String,
}

/// Enhanced status code per [RFC 3463] (`class.subject.detail`).
///
/// [RFC 3463]: https://datatracker.ietf.org/doc/html/rfc3463
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnhancedCode {
    /// Status class: `2` = success, `4` = transient, `5` = permanent.
    pub class: u8,
    /// Subject of the status — e.g. `7` for security/policy.
    pub subject: u16,
    /// Detail within the subject.
    pub detail: u16,
}

impl Response {
    /// Build a custom response. Most callers should use the named
    /// constructors below (e.g. [`Response::ok`], [`Response::mail_ok`]).
    pub fn new(code: u16, enhanced: Option<EnhancedCode>, message: impl Into<String>) -> Self {
        Self {
            code,
            enhanced,
            message: message.into(),
        }
    }

    /// Format as a single-line SMTP response, including trailing CRLF:
    /// `"code [enhanced] message\r\n"`.
    pub fn format(&self) -> String {
        let mut buf = String::new();
        write!(buf, "{}", self.code).unwrap();
        if let Some(ref e) = self.enhanced {
            write!(buf, " {}.{}.{}", e.class, e.subject, e.detail).unwrap();
        }
        write!(buf, " {}\r\n", self.message).unwrap();
        buf
    }

    /// Format as a greeting (no enhanced code): `"code message\r\n"`. Used
    /// for the initial `220` banner the server sends on connect.
    pub fn format_greeting(&self) -> String {
        format!("{} {}\r\n", self.code, self.message)
    }
}

/// Format a multi-line EHLO response. The first line carries the hostname;
/// each subsequent line carries one capability. Returns the full wire-format
/// bytes including CRLFs.
///
/// Pre-sized buffer + direct `push_str` instead of `write!`-macro
/// dispatch. Each line is `"250 " | "250-"` + the hostname or capability
/// + `\r\n` — all `push_str` of `&str` slices, no formatting machinery.
pub fn format_ehlo_response<S: AsRef<str>>(hostname: &str, capabilities: &[S]) -> String {
    // Capacity estimate: "250 " (4) + hostname + CRLF (2) + per-cap line.
    // Each cap line is "250-" or "250 " (4) + cap.len() + CRLF (2).
    let mut cap_len = 0usize;
    for cap in capabilities {
        cap_len += cap.as_ref().len() + 6;
    }
    let mut buf = String::with_capacity(hostname.len() + cap_len + 6);
    if capabilities.is_empty() {
        buf.push_str("250 ");
        buf.push_str(hostname);
        buf.push_str("\r\n");
    } else {
        buf.push_str("250-");
        buf.push_str(hostname);
        buf.push_str("\r\n");
        let last = capabilities.len() - 1;
        for (i, cap) in capabilities.iter().enumerate() {
            buf.push_str(if i == last { "250 " } else { "250-" });
            buf.push_str(cap.as_ref());
            buf.push_str("\r\n");
        }
    }
    buf
}

// well-known responses
impl Response {
    /// `220 <hostname> ESMTP MailRS` — connection greeting.
    pub fn greeting(hostname: &str) -> Self {
        Self::new(220, None, format!("{hostname} ESMTP MailRS"))
    }

    /// `250 2.0.0 OK` — generic EHLO success.
    pub fn ehlo_ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "OK")
    }

    /// `250 2.1.0 OK` — MAIL FROM accepted.
    pub fn mail_ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 1, detail: 0 }), "OK")
    }

    /// `250 2.1.5 OK` — RCPT TO accepted.
    pub fn rcpt_ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 1, detail: 5 }), "OK")
    }

    /// `354 Start mail input` — DATA accepted, awaiting message body.
    pub fn data_start() -> Self {
        Self::new(354, None, "Start mail input; end with <CRLF>.<CRLF>")
    }

    /// `250 2.0.0 OK: queued` — message body accepted.
    pub fn data_ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "OK: queued")
    }

    /// `221 2.0.0 Bye` — graceful close after QUIT.
    pub fn quit() -> Self {
        Self::new(221, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "Bye")
    }

    /// `503 5.5.1 Bad sequence of commands`.
    pub fn bad_sequence() -> Self {
        Self::new(503, Some(EnhancedCode { class: 5, subject: 5, detail: 1 }), "Bad sequence of commands")
    }

    /// `250 2.0.0 OK` — generic success.
    pub fn ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "OK")
    }

    /// `214 2.0.0 See https://...` — HELP reply.
    pub fn help() -> Self {
        Self::new(214, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "See https://tools.ietf.org/html/rfc5321")
    }

    /// `252 2.5.2 Cannot VRFY but will accept`.
    pub fn vrfy() -> Self {
        Self::new(252, Some(EnhancedCode { class: 2, subject: 5, detail: 2 }), "Cannot VRFY user, but will accept message")
    }

    /// `500 5.5.2 Syntax error, command unrecognized`.
    pub fn syntax_error() -> Self {
        Self::new(500, Some(EnhancedCode { class: 5, subject: 5, detail: 2 }), "Syntax error, command unrecognized")
    }

    /// `220 Ready to start TLS` — STARTTLS accepted (no enhanced code).
    pub fn tls_ready() -> Self {
        Self::new(220, None, "Ready to start TLS")
    }

    /// `334 <base64-msg>` — SASL authentication challenge.
    pub fn auth_challenge(msg: &str) -> Self {
        Self::new(334, None, msg.to_string())
    }

    /// `235 2.7.0 Authentication successful`.
    pub fn auth_ok() -> Self {
        Self::new(235, Some(EnhancedCode { class: 2, subject: 7, detail: 0 }), "Authentication successful")
    }

    /// `535 5.7.8 Authentication credentials invalid`.
    pub fn auth_failed() -> Self {
        Self::new(535, Some(EnhancedCode { class: 5, subject: 7, detail: 8 }), "Authentication credentials invalid")
    }

    /// `530 5.7.0 Must issue a STARTTLS command first`.
    pub fn tls_required() -> Self {
        Self::new(530, Some(EnhancedCode { class: 5, subject: 7, detail: 0 }), "Must issue a STARTTLS command first")
    }

    // anti-spam responses

    /// `554 5.7.1 Service unavailable; client blocked using <zone>` —
    /// DNSBL rejection.
    pub fn dnsbl_reject(zone: &str) -> Self {
        Self::new(
            554,
            Some(EnhancedCode { class: 5, subject: 7, detail: 1 }),
            format!("Service unavailable; client host blocked using {zone}"),
        )
    }

    /// `421 4.7.0 Too many connections, try again later` — transient rate
    /// limit rejection.
    pub fn rate_limited() -> Self {
        Self::new(
            421,
            Some(EnhancedCode { class: 4, subject: 7, detail: 0 }),
            "Too many connections, try again later",
        )
    }

    /// `450 4.7.1 Greylisted, please try again later` — transient greylist
    /// defer.
    pub fn greylisted() -> Self {
        Self::new(
            450,
            Some(EnhancedCode { class: 4, subject: 7, detail: 1 }),
            "Greylisted, please try again later",
        )
    }

    /// `550 5.7.23 SPF validation failed`.
    pub fn spf_reject() -> Self {
        Self::new(
            550,
            Some(EnhancedCode { class: 5, subject: 7, detail: 23 }),
            "SPF validation failed",
        )
    }

    /// `550 5.7.1 DMARC policy rejects this message`.
    pub fn dmarc_reject() -> Self {
        Self::new(
            550,
            Some(EnhancedCode { class: 5, subject: 7, detail: 1 }),
            "DMARC policy rejects this message",
        )
    }

    /// `552 5.3.4 Message size exceeds fixed maximum`.
    pub fn too_large() -> Self {
        Self::new(
            552,
            Some(EnhancedCode { class: 5, subject: 3, detail: 4 }),
            "Message size exceeds fixed maximum message size",
        )
    }

    /// `452 4.5.3 Too many recipients`.
    pub fn too_many_recipients() -> Self {
        Self::new(
            452,
            Some(EnhancedCode { class: 4, subject: 5, detail: 3 }),
            "Too many recipients",
        )
    }
}

#[cfg(test)]
mod tests;

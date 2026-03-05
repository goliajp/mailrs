use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub code: u16,
    pub enhanced: Option<EnhancedCode>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnhancedCode {
    pub class: u8,
    pub subject: u16,
    pub detail: u16,
}

impl Response {
    pub fn new(code: u16, enhanced: Option<EnhancedCode>, message: impl Into<String>) -> Self {
        Self {
            code,
            enhanced,
            message: message.into(),
        }
    }

    /// format as single-line SMTP response: "code [enhanced] message\r\n"
    pub fn format(&self) -> String {
        let mut buf = String::new();
        write!(buf, "{}", self.code).unwrap();
        if let Some(ref e) = self.enhanced {
            write!(buf, " {}.{}.{}", e.class, e.subject, e.detail).unwrap();
        }
        write!(buf, " {}\r\n", self.message).unwrap();
        buf
    }

    /// format greeting (no enhanced code): "220 message\r\n"
    pub fn format_greeting(&self) -> String {
        format!("{} {}\r\n", self.code, self.message)
    }
}

/// format EHLO multiline response
pub fn format_ehlo_response<S: AsRef<str>>(hostname: &str, capabilities: &[S]) -> String {
    let mut buf = String::new();
    if capabilities.is_empty() {
        write!(buf, "250 {}\r\n", hostname).unwrap();
    } else {
        write!(buf, "250-{}\r\n", hostname).unwrap();
        for (i, cap) in capabilities.iter().enumerate() {
            if i == capabilities.len() - 1 {
                write!(buf, "250 {}\r\n", cap.as_ref()).unwrap();
            } else {
                write!(buf, "250-{}\r\n", cap.as_ref()).unwrap();
            }
        }
    }
    buf
}

// well-known responses
impl Response {
    pub fn greeting(hostname: &str) -> Self {
        Self::new(220, None, format!("{hostname} ESMTP MailRS"))
    }

    pub fn ehlo_ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "OK")
    }

    pub fn mail_ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 1, detail: 0 }), "OK")
    }

    pub fn rcpt_ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 1, detail: 5 }), "OK")
    }

    pub fn data_start() -> Self {
        Self::new(354, None, "Start mail input; end with <CRLF>.<CRLF>")
    }

    pub fn data_ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "OK: queued")
    }

    pub fn quit() -> Self {
        Self::new(221, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "Bye")
    }

    pub fn bad_sequence() -> Self {
        Self::new(503, Some(EnhancedCode { class: 5, subject: 5, detail: 1 }), "Bad sequence of commands")
    }

    pub fn ok() -> Self {
        Self::new(250, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "OK")
    }

    pub fn help() -> Self {
        Self::new(214, Some(EnhancedCode { class: 2, subject: 0, detail: 0 }), "See https://tools.ietf.org/html/rfc5321")
    }

    pub fn vrfy() -> Self {
        Self::new(252, Some(EnhancedCode { class: 2, subject: 5, detail: 2 }), "Cannot VRFY user, but will accept message")
    }

    pub fn syntax_error() -> Self {
        Self::new(500, Some(EnhancedCode { class: 5, subject: 5, detail: 2 }), "Syntax error, command unrecognized")
    }

    pub fn tls_ready() -> Self {
        Self::new(220, None, "Ready to start TLS")
    }

    pub fn auth_challenge(msg: &str) -> Self {
        Self::new(334, None, msg.to_string())
    }

    pub fn auth_ok() -> Self {
        Self::new(235, Some(EnhancedCode { class: 2, subject: 7, detail: 0 }), "Authentication successful")
    }

    pub fn auth_failed() -> Self {
        Self::new(535, Some(EnhancedCode { class: 5, subject: 7, detail: 8 }), "Authentication credentials invalid")
    }

    pub fn tls_required() -> Self {
        Self::new(530, Some(EnhancedCode { class: 5, subject: 7, detail: 0 }), "Must issue a STARTTLS command first")
    }

    // anti-spam responses

    pub fn dnsbl_reject(zone: &str) -> Self {
        Self::new(
            554,
            Some(EnhancedCode { class: 5, subject: 7, detail: 1 }),
            format!("Service unavailable; client host blocked using {zone}"),
        )
    }

    pub fn rate_limited() -> Self {
        Self::new(
            421,
            Some(EnhancedCode { class: 4, subject: 7, detail: 0 }),
            "Too many connections, try again later",
        )
    }

    pub fn greylisted() -> Self {
        Self::new(
            450,
            Some(EnhancedCode { class: 4, subject: 7, detail: 1 }),
            "Greylisted, please try again later",
        )
    }

    pub fn spf_reject() -> Self {
        Self::new(
            550,
            Some(EnhancedCode { class: 5, subject: 7, detail: 23 }),
            "SPF validation failed",
        )
    }

    pub fn dmarc_reject() -> Self {
        Self::new(
            550,
            Some(EnhancedCode { class: 5, subject: 7, detail: 1 }),
            "DMARC policy rejects this message",
        )
    }

    pub fn too_large() -> Self {
        Self::new(
            552,
            Some(EnhancedCode { class: 5, subject: 3, detail: 4 }),
            "Message size exceeds fixed maximum message size",
        )
    }

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

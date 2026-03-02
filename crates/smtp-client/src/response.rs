/// client-side SMTP response parsing

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpResponse {
    pub code: u16,
    pub lines: Vec<String>,
}

impl SmtpResponse {
    pub fn is_positive(&self) -> bool {
        (200..400).contains(&self.code)
    }

    pub fn is_transient_error(&self) -> bool {
        (400..500).contains(&self.code)
    }

    pub fn is_permanent_error(&self) -> bool {
        self.code >= 500
    }

    pub fn message(&self) -> String {
        self.lines.join("\n")
    }
}

/// parse a single or multiline SMTP response from raw text
/// returns None if the response is incomplete
pub fn parse_response(input: &str) -> Option<SmtpResponse> {
    let mut code = None;
    let mut lines = Vec::new();

    for line in input.lines() {
        if line.len() < 3 {
            return None;
        }

        let line_code: u16 = line[..3].parse().ok()?;

        if let Some(c) = code {
            if c != line_code {
                return None;
            }
        } else {
            code = Some(line_code);
        }

        let separator = line.as_bytes().get(3).copied();
        let text = if line.len() > 4 { &line[4..] } else { "" };
        lines.push(text.to_string());

        // ' ' = last line, '-' = continuation
        match separator {
            Some(b' ') | None => {
                return Some(SmtpResponse {
                    code: code?,
                    lines,
                });
            }
            Some(b'-') => continue,
            _ => return None,
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_line() {
        let r = parse_response("250 OK\r\n").unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines, vec!["OK"]);
        assert!(r.is_positive());
    }

    #[test]
    fn parse_multiline() {
        let input = "250-mx.example.com\r\n250-PIPELINING\r\n250 SIZE 10240000";
        let r = parse_response(input).unwrap();
        assert_eq!(r.code, 250);
        assert_eq!(r.lines.len(), 3);
        assert_eq!(r.lines[0], "mx.example.com");
        assert_eq!(r.lines[2], "SIZE 10240000");
    }

    #[test]
    fn transient_error() {
        let r = parse_response("421 Try again later").unwrap();
        assert!(r.is_transient_error());
        assert!(!r.is_positive());
    }

    #[test]
    fn permanent_error() {
        let r = parse_response("550 User not found").unwrap();
        assert!(r.is_permanent_error());
    }

    #[test]
    fn incomplete_returns_none() {
        assert!(parse_response("").is_none());
    }
}

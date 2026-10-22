/// detect if an SMTP error is a permanent/hard bounce (5xx)
pub fn is_hard_bounce(error: &str) -> bool {
    let trimmed = error.trim();
    trimmed.starts_with('5') || trimmed.starts_with("5.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_bounce_detects_5xx() {
        assert!(is_hard_bounce("550 mailbox unavailable"));
        assert!(is_hard_bounce("552 message too large"));
        assert!(is_hard_bounce("5.7.1 policy reject"));
    }

    #[test]
    fn hard_bounce_rejects_4xx_and_other() {
        assert!(!is_hard_bounce("450 try again"));
        assert!(!is_hard_bounce("421 service unavailable"));
        assert!(!is_hard_bounce("250 ok"));
        assert!(!is_hard_bounce(""));
        assert!(!is_hard_bounce("random text"));
    }

    #[test]
    fn hard_bounce_trims_leading_whitespace() {
        assert!(is_hard_bounce("  550 spaces"));
        assert!(!is_hard_bounce("  450 spaces"));
    }
}

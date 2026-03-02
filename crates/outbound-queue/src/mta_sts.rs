/// check if an MX hostname matches any pattern in the MTA-STS policy
pub fn mx_matches_policy(mx_host: &str, policy_mx: &[&str]) -> bool {
    let mx_lower = mx_host.to_lowercase();
    for pattern in policy_mx {
        let p = pattern.to_lowercase();
        if p.starts_with("*.") {
            // wildcard match: *.example.com matches mail.example.com
            let suffix = &p[1..]; // ".example.com"
            if mx_lower.ends_with(&suffix) && mx_lower.len() > suffix.len() {
                // ensure the part before the suffix has no dots (single level)
                let prefix = &mx_lower[..mx_lower.len() - suffix.len()];
                if !prefix.contains('.') {
                    return true;
                }
            }
        } else if mx_lower == p {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        assert!(mx_matches_policy("mail.example.com", &["mail.example.com"]));
    }

    #[test]
    fn wildcard_match() {
        assert!(mx_matches_policy("mail.example.com", &["*.example.com"]));
    }

    #[test]
    fn wildcard_no_match_different_domain() {
        assert!(!mx_matches_policy("other.com", &["*.example.com"]));
    }

    #[test]
    fn wildcard_no_match_subdomain() {
        // *.example.com should NOT match sub.mail.example.com
        assert!(!mx_matches_policy("sub.mail.example.com", &["*.example.com"]));
    }

    #[test]
    fn case_insensitive() {
        assert!(mx_matches_policy("MAIL.Example.COM", &["*.example.com"]));
    }

    #[test]
    fn no_match() {
        assert!(!mx_matches_policy("mail.other.com", &["*.example.com"]));
    }

    #[test]
    fn multiple_patterns() {
        assert!(mx_matches_policy(
            "backup.example.com",
            &["mail.example.com", "*.example.com"]
        ));
    }
}

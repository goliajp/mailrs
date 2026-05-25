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
        assert!(!mx_matches_policy(
            "sub.mail.example.com",
            &["*.example.com"]
        ));
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

    #[test]
    fn empty_policy_never_matches() {
        assert!(!mx_matches_policy("mail.example.com", &[]));
    }

    #[test]
    fn wildcard_does_not_match_bare_domain() {
        // *.example.com should not match "example.com" itself
        assert!(!mx_matches_policy("example.com", &["*.example.com"]));
    }

    #[test]
    fn exact_match_case_insensitive() {
        assert!(mx_matches_policy("MAIL.EXAMPLE.COM", &["mail.example.com"]));
        assert!(mx_matches_policy("mail.example.com", &["MAIL.EXAMPLE.COM"]));
    }

    #[test]
    fn wildcard_matches_single_label_only() {
        // only one label before the suffix may match
        assert!(mx_matches_policy("a.example.com", &["*.example.com"]));
        assert!(!mx_matches_policy("a.b.example.com", &["*.example.com"]));
    }

    #[test]
    fn no_false_positive_partial_suffix() {
        // "notexample.com" should not match "*.example.com"
        assert!(!mx_matches_policy("mailnotexample.com", &["*.example.com"]));
    }

    #[test]
    fn first_matching_pattern_wins() {
        // result is true regardless of order when first pattern matches
        assert!(mx_matches_policy(
            "mail.example.com",
            &["mail.example.com", "*.other.com"]
        ));
    }

    #[test]
    fn pattern_without_wildcard_no_partial_match() {
        // "mail.example.com" should not match pattern "example.com"
        assert!(!mx_matches_policy("mail.example.com", &["example.com"]));
    }

    #[test]
    fn wildcard_pattern_case_insensitive_uppercase() {
        assert!(mx_matches_policy("mail.example.com", &["*.EXAMPLE.COM"]));
    }

    #[test]
    fn wildcard_pattern_mixed_case() {
        assert!(mx_matches_policy("Mail.Example.Com", &["*.example.com"]));
        assert!(mx_matches_policy("mail.example.com", &["*.Example.Com"]));
    }

    #[test]
    fn wildcard_does_not_match_empty_prefix() {
        // ".example.com" should not match *.example.com
        // the prefix before the suffix must have length > 0, which the code handles via `mx_lower.len() > suffix.len()`
        assert!(!mx_matches_policy(".example.com", &["*.example.com"]));
    }

    #[test]
    fn exact_match_trailing_case_sensitivity() {
        assert!(mx_matches_policy("mx1.gmail.com", &["mx1.gmail.com"]));
        assert!(mx_matches_policy("MX1.GMAIL.COM", &["mx1.gmail.com"]));
    }

    #[test]
    fn wildcard_single_char_prefix() {
        assert!(mx_matches_policy("a.example.com", &["*.example.com"]));
    }

    #[test]
    fn wildcard_long_prefix() {
        assert!(mx_matches_policy(
            "very-long-hostname-label.example.com",
            &["*.example.com"]
        ));
    }

    #[test]
    fn wildcard_hyphenated_prefix() {
        assert!(mx_matches_policy(
            "mail-relay-01.example.com",
            &["*.example.com"]
        ));
    }

    #[test]
    fn wildcard_numeric_prefix() {
        assert!(mx_matches_policy("123.example.com", &["*.example.com"]));
    }

    #[test]
    fn multiple_patterns_first_exact_wins() {
        assert!(mx_matches_policy(
            "mx.example.com",
            &["mx.example.com", "*.other.com"]
        ));
    }

    #[test]
    fn multiple_patterns_second_wildcard_wins() {
        assert!(mx_matches_policy(
            "relay.other.com",
            &["mx.example.com", "*.other.com"]
        ));
    }

    #[test]
    fn multiple_patterns_none_match() {
        assert!(!mx_matches_policy(
            "mail.unknown.org",
            &["mx.example.com", "*.other.com"]
        ));
    }

    #[test]
    fn wildcard_with_deeper_subdomain_parent() {
        // *.mail.example.com should match relay.mail.example.com
        assert!(mx_matches_policy(
            "relay.mail.example.com",
            &["*.mail.example.com"]
        ));
    }

    #[test]
    fn wildcard_with_deeper_subdomain_no_double() {
        // *.mail.example.com should NOT match a.b.mail.example.com
        assert!(!mx_matches_policy(
            "a.b.mail.example.com",
            &["*.mail.example.com"]
        ));
    }

    #[test]
    fn wildcard_pattern_exact_domain_not_matched() {
        // *.mail.example.com should NOT match mail.example.com itself
        assert!(!mx_matches_policy(
            "mail.example.com",
            &["*.mail.example.com"]
        ));
    }

    #[test]
    fn pattern_with_only_wildcard_star_dot() {
        // edge case: pattern "*.com" should match "anything.com"
        assert!(mx_matches_policy("anything.com", &["*.com"]));
    }

    #[test]
    fn pattern_star_dot_does_not_match_subdomain() {
        // "*.com" should not match "a.b.com"
        assert!(!mx_matches_policy("a.b.com", &["*.com"]));
    }

    #[test]
    fn empty_mx_host_never_matches() {
        assert!(!mx_matches_policy("", &["*.example.com"]));
        assert!(!mx_matches_policy("", &["example.com"]));
    }

    #[test]
    fn empty_mx_host_and_empty_policy() {
        assert!(!mx_matches_policy("", &[]));
    }

    #[test]
    fn exact_match_does_not_match_substring() {
        // exact pattern "example.com" should not match "notexample.com"
        assert!(!mx_matches_policy("notexample.com", &["example.com"]));
    }

    #[test]
    fn exact_match_does_not_match_superdomain() {
        assert!(!mx_matches_policy("example.com", &["sub.example.com"]));
    }

    #[test]
    fn many_patterns_performance() {
        // ensure linear scan with many patterns still works
        let patterns: Vec<String> = (0..100)
            .map(|i| format!("mx{}.example{}.com", i, i))
            .collect();
        let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
        assert!(!mx_matches_policy("mx.unknown.com", &pattern_refs));
        assert!(mx_matches_policy("mx50.example50.com", &pattern_refs));
    }

    #[test]
    fn wildcard_many_patterns_last_matches() {
        let mut patterns: Vec<String> =
            (0..99).map(|i| format!("mx{}.wrong{}.com", i, i)).collect();
        patterns.push("*.target.com".to_string());
        let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
        assert!(mx_matches_policy("relay.target.com", &pattern_refs));
    }

    #[test]
    fn real_world_google_mx_patterns() {
        // google's MTA-STS policy uses *.gmail-smtp-in.l.google.com
        let policy = &["gmail-smtp-in.l.google.com", "*.gmail-smtp-in.l.google.com"];
        assert!(mx_matches_policy("gmail-smtp-in.l.google.com", policy));
        assert!(mx_matches_policy("alt1.gmail-smtp-in.l.google.com", policy));
        // wildcard only matches single level — *.google.com would NOT match multi-level hostnames
        let broad_policy = &["*.google.com"];
        assert!(!mx_matches_policy(
            "alt1.gmail-smtp-in.l.google.com",
            broad_policy
        ));
    }

    #[test]
    fn real_world_microsoft_mx_patterns() {
        let policy = &["*.mail.protection.outlook.com"];
        assert!(mx_matches_policy(
            "contoso-com.mail.protection.outlook.com",
            policy
        ));
        assert!(!mx_matches_policy(
            "a.b.mail.protection.outlook.com",
            policy
        ));
    }
}

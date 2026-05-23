//! Enforcement helpers: MX pattern matching + the
//! `enforce(policy, mx)` decision function.

use crate::policy::{Policy, PolicyMode};

/// Decision returned by [`enforce`] for a single MX hostname against a
/// parsed STS policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// MX is allowed under this policy. Caller proceeds with the
    /// normal STARTTLS + certificate-verification delivery path.
    Allow,
    /// `mode: enforce` and the MX is not in `mx:`. Caller must NOT
    /// deliver to this MX (RFC 8461 §5).
    Deny,
    /// `mode: testing` or `mode: none`, or no policy applies — the
    /// caller may still deliver, but if `mode: testing` it should
    /// also emit a TLS-RPT failure record so the recipient domain
    /// learns the MX list is stale.
    NoPolicy,
}

/// Build the well-known URL for `domain`'s policy file.
///
/// Per RFC 8461 §3.3, every receiving domain advertising STS exposes
/// its policy at `https://mta-sts.<domain>/.well-known/mta-sts.txt`.
///
/// ```
/// use mailrs_mta_sts::policy_url;
/// assert_eq!(
///     policy_url("example.com"),
///     "https://mta-sts.example.com/.well-known/mta-sts.txt"
/// );
/// ```
pub fn policy_url(domain: &str) -> String {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    format!("https://mta-sts.{domain}/.well-known/mta-sts.txt")
}

/// Match an MX hostname against an MTA-STS `mx:` pattern.
///
/// RFC 8461 §4.1 patterns:
/// - Literal: `mail.example.com` matches only `mail.example.com`.
/// - Wildcard: `*.example.com` matches exactly one DNS label below
///   `example.com` (so `a.example.com` matches, but
///   `a.b.example.com` does NOT).
///
/// Comparison is case-insensitive; both sides are normalised by
/// lowercasing + trimming a trailing dot.
///
/// ```
/// use mailrs_mta_sts::mx_matches;
/// assert!(mx_matches("mail.example.com", "mail.example.com"));
/// assert!(mx_matches("a.example.com", "*.example.com"));
/// assert!(!mx_matches("a.b.example.com", "*.example.com"));
/// assert!(!mx_matches("attacker.com", "*.example.com"));
/// ```
pub fn mx_matches(mx_host: &str, pattern: &str) -> bool {
    let mx = mx_host.trim().trim_end_matches('.').to_ascii_lowercase();
    let pat = pattern.trim().trim_end_matches('.').to_ascii_lowercase();

    if let Some(suffix) = pat.strip_prefix("*.") {
        // Wildcard: must match exactly one label below the suffix.
        let Some(rest) = mx.strip_suffix(&format!(".{suffix}")) else {
            return false;
        };
        // `rest` is the single-label prefix; must contain no '.'.
        !rest.is_empty() && !rest.contains('.')
    } else {
        mx == pat
    }
}

/// Apply `policy` to a single MX hostname.
///
/// Iterates the policy's `mx:` patterns in order; the first match
/// returns `Allow`. If none match, the decision depends on `mode`:
///
/// - `Enforce` → `Deny`
/// - `Testing` → `NoPolicy` (caller delivers but should emit a
///   TLS-RPT failure report)
/// - `None` → `NoPolicy` (policy disabled by recipient)
///
/// Pass each MX returned by DNS through this function; if **any** MX
/// returns `Allow` the caller may deliver to that MX. The first
/// `Deny` MX is the strict "do not deliver" signal.
pub fn enforce(policy: &Policy, mx_host: &str) -> Decision {
    let matches = policy.mx.iter().any(|p| mx_matches(mx_host, p));
    if matches {
        return Decision::Allow;
    }
    match policy.mode {
        PolicyMode::Enforce => Decision::Deny,
        PolicyMode::Testing | PolicyMode::None => Decision::NoPolicy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(mode: PolicyMode, mx: &[&str]) -> Policy {
        Policy {
            mode,
            mx: mx.iter().map(|s| s.to_string()).collect(),
            max_age: 86400,
        }
    }

    #[test]
    fn policy_url_basic() {
        assert_eq!(
            policy_url("example.com"),
            "https://mta-sts.example.com/.well-known/mta-sts.txt"
        );
    }

    #[test]
    fn policy_url_lowercases_and_strips_trailing_dot() {
        assert_eq!(
            policy_url("Example.COM."),
            "https://mta-sts.example.com/.well-known/mta-sts.txt"
        );
    }

    #[test]
    fn mx_literal_match() {
        assert!(mx_matches("mail.example.com", "mail.example.com"));
    }

    #[test]
    fn mx_literal_no_match() {
        assert!(!mx_matches("attacker.com", "mail.example.com"));
    }

    #[test]
    fn mx_case_insensitive() {
        assert!(mx_matches("Mail.Example.COM", "mail.example.com"));
        assert!(mx_matches("mail.example.com", "MAIL.EXAMPLE.COM"));
    }

    #[test]
    fn mx_trailing_dot_ignored() {
        assert!(mx_matches("mail.example.com.", "mail.example.com"));
        assert!(mx_matches("mail.example.com", "mail.example.com."));
    }

    #[test]
    fn mx_wildcard_matches_one_label() {
        assert!(mx_matches("a.example.com", "*.example.com"));
        assert!(mx_matches("mail.example.com", "*.example.com"));
    }

    #[test]
    fn mx_wildcard_does_not_match_two_labels() {
        // RFC 8461 §4.1: wildcard matches exactly one label.
        assert!(!mx_matches("a.b.example.com", "*.example.com"));
    }

    #[test]
    fn mx_wildcard_does_not_match_bare_suffix() {
        // `*.example.com` does NOT match `example.com` itself.
        assert!(!mx_matches("example.com", "*.example.com"));
    }

    #[test]
    fn mx_wildcard_does_not_match_different_domain() {
        assert!(!mx_matches("a.attacker.com", "*.example.com"));
    }

    #[test]
    fn enforce_allows_matching_mx() {
        let p = policy(PolicyMode::Enforce, &["mail.example.com"]);
        assert_eq!(enforce(&p, "mail.example.com"), Decision::Allow);
    }

    #[test]
    fn enforce_denies_unmatched_mx_in_enforce_mode() {
        let p = policy(PolicyMode::Enforce, &["mail.example.com"]);
        assert_eq!(enforce(&p, "rogue.example.com"), Decision::Deny);
    }

    #[test]
    fn enforce_returns_nopolicy_in_testing_mode_on_unmatched() {
        let p = policy(PolicyMode::Testing, &["mail.example.com"]);
        assert_eq!(enforce(&p, "rogue.example.com"), Decision::NoPolicy);
    }

    #[test]
    fn enforce_returns_nopolicy_when_mode_none() {
        let p = policy(PolicyMode::None, &["mail.example.com"]);
        assert_eq!(enforce(&p, "rogue.example.com"), Decision::NoPolicy);
        // Even in mode=none, a matching MX returns Allow — semantically
        // identical to NoPolicy at the caller, but more informative.
        assert_eq!(enforce(&p, "mail.example.com"), Decision::Allow);
    }

    #[test]
    fn enforce_wildcard_match_in_enforce_mode() {
        let p = policy(PolicyMode::Enforce, &["*.example.com"]);
        assert_eq!(enforce(&p, "mx1.example.com"), Decision::Allow);
        assert_eq!(enforce(&p, "a.b.example.com"), Decision::Deny);
    }

    #[test]
    fn enforce_first_matching_pattern_wins() {
        let p = policy(PolicyMode::Enforce, &["backup.example", "*.example.com"]);
        // Both literal and wildcard contribute — order doesn't matter
        // for correctness, but exercises the iter().any() short-circuit.
        assert_eq!(enforce(&p, "backup.example"), Decision::Allow);
        assert_eq!(enforce(&p, "any.example.com"), Decision::Allow);
        assert_eq!(enforce(&p, "evil.com"), Decision::Deny);
    }
}

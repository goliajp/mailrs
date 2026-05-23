//! DMARC identifier alignment (RFC 7489 §3.1).
//!
//! Alignment is the rule that lets DMARC inherit SPF and DKIM verdicts:
//!
//! * **DKIM identifier alignment** — the `d=` domain on a *passing* DKIM
//!   signature must align with the From: header domain (the "RFC5322.From"
//!   identifier).
//! * **SPF identifier alignment** — the MAIL FROM domain (or HELO, when
//!   MAIL FROM is empty) used by SPF must align with the From: header
//!   domain.
//!
//! Two modes, both per RFC 7489 §3.1.x:
//!
//! * **Strict (`s`)** — fully-qualified names must match exactly
//!   (case-insensitive).
//! * **Relaxed (`r`, default)** — organizational domains must match.
//!   "Organizational domain" comes from the Public Suffix List: it's
//!   the part one level below the effective TLD (e.g. `example.co.uk`
//!   from `mail.example.co.uk`).

use crate::policy::Alignment;

/// Result of an alignment check, captured for the Authentication-Results
/// header + aggregate report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentOutcome {
    /// Both domains were valid and aligned per the requested mode.
    Aligned,
    /// Both domains were valid but failed alignment (different orgs in
    /// relaxed mode, different FQDNs in strict mode).
    NotAligned,
    /// One of the inputs wasn't a parseable domain (empty / PSL refused).
    InvalidDomain,
}

impl AlignmentOutcome {
    /// Convenience: `Aligned` is the only "pass" outcome.
    pub fn is_aligned(self) -> bool {
        matches!(self, AlignmentOutcome::Aligned)
    }
}

/// Check whether `auth_domain` (the SPF MAIL FROM or DKIM `d=` value)
/// aligns with `from_domain` (the RFC 5322 `From:` header domain) under
/// the requested [`Alignment`] mode.
pub fn check(auth_domain: &str, from_domain: &str, mode: Alignment) -> AlignmentOutcome {
    if auth_domain.is_empty() || from_domain.is_empty() {
        return AlignmentOutcome::InvalidDomain;
    }
    let a = auth_domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let f = from_domain.trim().trim_end_matches('.').to_ascii_lowercase();

    match mode {
        Alignment::Strict => {
            if a == f {
                AlignmentOutcome::Aligned
            } else {
                AlignmentOutcome::NotAligned
            }
        }
        Alignment::Relaxed => {
            let Some(a_org) = organizational_domain(&a) else {
                return AlignmentOutcome::InvalidDomain;
            };
            let Some(f_org) = organizational_domain(&f) else {
                return AlignmentOutcome::InvalidDomain;
            };
            if a_org == f_org {
                AlignmentOutcome::Aligned
            } else {
                AlignmentOutcome::NotAligned
            }
        }
    }
}

/// Compute the organizational domain via the Public Suffix List.
///
/// Returns `None` if the input isn't a parseable domain (e.g. bare IP,
/// no labels at all, PSL doesn't recognize the suffix).
///
/// Examples (from the embedded PSL):
/// * `mail.example.com` → `example.com`
/// * `news.example.co.uk` → `example.co.uk`
/// * `127.0.0.1` → `None`
pub fn organizational_domain(domain: &str) -> Option<String> {
    let domain = domain.trim().trim_end_matches('.');
    if domain.is_empty() {
        return None;
    }
    // psl::domain takes bytes and returns the registrable domain.
    let parsed = psl::domain(domain.as_bytes())?;
    let bytes = parsed.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_alignment_exact_match() {
        let r = check("example.com", "example.com", Alignment::Strict);
        assert_eq!(r, AlignmentOutcome::Aligned);
    }

    #[test]
    fn strict_alignment_subdomain_fails() {
        let r = check("mail.example.com", "example.com", Alignment::Strict);
        assert_eq!(r, AlignmentOutcome::NotAligned);
    }

    #[test]
    fn strict_alignment_case_insensitive() {
        let r = check("Example.COM", "example.com", Alignment::Strict);
        assert_eq!(r, AlignmentOutcome::Aligned);
    }

    #[test]
    fn strict_alignment_trailing_dot_ignored() {
        let r = check("example.com.", "example.com", Alignment::Strict);
        assert_eq!(r, AlignmentOutcome::Aligned);
    }

    #[test]
    fn relaxed_alignment_subdomain_passes() {
        let r = check("mail.example.com", "example.com", Alignment::Relaxed);
        assert_eq!(r, AlignmentOutcome::Aligned);
    }

    #[test]
    fn relaxed_alignment_different_orgs_fails() {
        let r = check("mail.attacker.com", "example.com", Alignment::Relaxed);
        assert_eq!(r, AlignmentOutcome::NotAligned);
    }

    #[test]
    fn relaxed_alignment_handles_psl_double_suffix() {
        // example.co.uk has a 2-label suffix; PSL knows.
        let r = check("mail.example.co.uk", "example.co.uk", Alignment::Relaxed);
        assert_eq!(r, AlignmentOutcome::Aligned);
    }

    #[test]
    fn relaxed_alignment_subdomain_under_double_suffix() {
        let r = check(
            "smtp.mail.example.co.uk",
            "www.example.co.uk",
            Alignment::Relaxed,
        );
        assert_eq!(r, AlignmentOutcome::Aligned);
    }

    #[test]
    fn empty_auth_domain_is_invalid() {
        let r = check("", "example.com", Alignment::Relaxed);
        assert_eq!(r, AlignmentOutcome::InvalidDomain);
    }

    #[test]
    fn empty_from_domain_is_invalid() {
        let r = check("example.com", "", Alignment::Relaxed);
        assert_eq!(r, AlignmentOutcome::InvalidDomain);
    }

    #[test]
    fn organizational_domain_basic() {
        assert_eq!(
            organizational_domain("mail.example.com").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn organizational_domain_co_uk() {
        assert_eq!(
            organizational_domain("news.example.co.uk").as_deref(),
            Some("example.co.uk")
        );
    }

    #[test]
    fn organizational_domain_already_orgdomain() {
        assert_eq!(
            organizational_domain("example.com").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn organizational_domain_empty_returns_none() {
        assert_eq!(organizational_domain("").as_deref(), None);
    }

    #[test]
    fn is_aligned_helper() {
        assert!(AlignmentOutcome::Aligned.is_aligned());
        assert!(!AlignmentOutcome::NotAligned.is_aligned());
        assert!(!AlignmentOutcome::InvalidDomain.is_aligned());
    }
}

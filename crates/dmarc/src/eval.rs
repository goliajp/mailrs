//! DMARC evaluation (RFC 7489 §6.6).
//!
//! Combines an SPF result, zero or more DKIM-Signature results, and a
//! published [`DmarcPolicy`] into a final DMARC outcome:
//!
//! ```text
//! aligned_spf_pass   = (SPF result == pass)  AND  spf_domain aligned with from_domain
//! aligned_dkim_pass  = any DKIM signature with (result == pass AND d= aligned with from_domain)
//! dmarc_pass         = aligned_spf_pass OR aligned_dkim_pass
//!
//! disposition = dmarc_pass ? None
//!                          : (subdomain? policy.subdomain_policy : policy.policy)
//!                            modulated by pct=
//! ```
//!
//! Notes:
//! * **`pct=` sampling** isn't done here — we surface the policy and let
//!   the caller's RNG decide. (Stateless eval keeps the function pure.)
//! * **No DNS lookup** here. Caller resolves the TXT record and hands
//!   the parsed [`DmarcPolicy`] in.
//! * **Subdomain detection** — if `from_domain != policy_domain`, we
//!   apply `subdomain_policy` instead of `policy`.

use crate::align::check as align_check;
use compact_str::CompactString;

use crate::policy::{DmarcPolicy, PolicyAction};

/// One DKIM signature's verification verdict + identifying domain.
#[derive(Debug, Clone)]
pub struct DkimSignatureResult {
    /// `d=` value from the DKIM-Signature header.
    ///
    /// **v2 change**: `CompactString` — matches `mailrs-dkim::DkimHeader.domain`'s
    /// type so the inbound pipeline can clone through without re-allocating.
    pub d_domain: CompactString,
    /// Whether this signature verified (RSA-SHA256 / Ed25519-SHA256).
    /// Per RFC 7489 §3.1.1, only `pass` results contribute to DMARC.
    pub pass: bool,
}

/// SPF verification context for DMARC.
#[derive(Debug, Clone)]
pub struct SpfResult {
    /// The MAIL FROM domain used in the SPF check (or HELO when MAIL FROM was empty).
    ///
    /// **v2 change**: `CompactString` — see `DkimSignatureResult.d_domain`.
    pub domain: CompactString,
    /// Whether the SPF result was `pass`.
    pub pass: bool,
}

/// Input bundle for [`evaluate`]. All fields are owned because the
/// outcome's `reason` strings borrow from them in some implementations.
#[derive(Debug, Clone)]
pub struct DmarcInput {
    /// RFC 5322 `From:` header domain — the identity DMARC anchors on.
    pub from_domain: CompactString,
    /// The domain whose `_dmarc.<domain>` TXT we used. Equal to `from_domain`
    /// when the From: domain has a policy directly; otherwise the org domain.
    pub policy_domain: CompactString,
    /// SPF result (or absent if SPF wasn't checked / errored).
    pub spf: Option<SpfResult>,
    /// All DKIM signatures observed on the message.
    pub dkim: Vec<DkimSignatureResult>,
}

/// DMARC outcome including the per-authn alignment outcomes (used for
/// the `Authentication-Results` header + aggregate reports).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmarcOutcome {
    /// `true` when SPF passed and its domain aligned with From.
    pub aligned_spf_pass: bool,
    /// `true` when at least one DKIM signature passed AND its d=
    /// aligned with From.
    pub aligned_dkim_pass: bool,
    /// The DMARC verdict — `pass` if either aligned-auth passed.
    pub dmarc_pass: bool,
    /// Disposition the policy specifies for this message.
    /// `pass` → always `None`; `fail` → the policy's `p=`/`sp=` choice.
    /// Caller should still apply `pct=` sampling before enforcing.
    pub disposition: PolicyAction,
    /// Human-readable reason fragment for the AuthResults header
    /// (`policy.dmarc=fail (p=reject, sp=quarantine, ...)`).
    pub reason: String,
    /// Echo of the policy's `pct=` value, so the caller can sample.
    pub pct: u8,
}

/// Per RFC 7489 §6.6.3, the disposition is determined by whether the
/// From: domain matches the organizational domain (`p=`) or is a
/// subdomain of it (`sp=`).
fn pick_disposition(input: &DmarcInput, policy: &DmarcPolicy) -> PolicyAction {
    let from = input
        .from_domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let pol = input
        .policy_domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if from == pol {
        policy.policy
    } else {
        policy.subdomain_policy
    }
}

/// Evaluate DMARC for one message.
///
/// Pure function: no DNS, no clock, no RNG. Determinism in, determinism out.
///
/// # Example
///
/// ```
/// use mailrs_dmarc::eval::{evaluate, DkimSignatureResult, DmarcInput, SpfResult};
/// use mailrs_dmarc::policy::{DmarcPolicy, PolicyAction};
///
/// let policy = DmarcPolicy::parse("v=DMARC1; p=reject").unwrap();
/// let input = DmarcInput {
///     from_domain: "alice@example.com".rsplit('@').next().unwrap().into(),
///     policy_domain: "example.com".into(),
///     spf: Some(SpfResult { domain: "mail.example.com".into(), pass: true }),
///     dkim: vec![],
/// };
/// let outcome = evaluate(&policy, &input);
/// assert!(outcome.aligned_spf_pass);
/// assert!(outcome.dmarc_pass);
/// assert_eq!(outcome.disposition, PolicyAction::None);
/// ```
pub fn evaluate(policy: &DmarcPolicy, input: &DmarcInput) -> DmarcOutcome {
    // Aligned SPF: pass AND aligned under `aspf` mode.
    let aligned_spf_pass = match input.spf.as_ref() {
        Some(spf) if spf.pass => {
            align_check(&spf.domain, &input.from_domain, policy.aspf).is_aligned()
        }
        _ => false,
    };

    // Aligned DKIM: any signature pass-and-aligned wins.
    let aligned_dkim_pass = input.dkim.iter().any(|sig| {
        sig.pass && align_check(&sig.d_domain, &input.from_domain, policy.adkim).is_aligned()
    });

    let dmarc_pass = aligned_spf_pass || aligned_dkim_pass;

    let disposition = if dmarc_pass {
        PolicyAction::None
    } else {
        pick_disposition(input, policy)
    };

    let reason = format_reason(policy, input, aligned_spf_pass, aligned_dkim_pass);

    DmarcOutcome {
        aligned_spf_pass,
        aligned_dkim_pass,
        dmarc_pass,
        disposition,
        reason,
        pct: policy.pct,
    }
}

fn format_reason(
    policy: &DmarcPolicy,
    input: &DmarcInput,
    spf_pass: bool,
    dkim_pass: bool,
) -> String {
    let mut s = String::with_capacity(64);
    if spf_pass {
        s.push_str("aligned-spf=pass");
    } else if let Some(spf) = input.spf.as_ref() {
        s.push_str(if spf.pass {
            "aligned-spf=misaligned"
        } else {
            "aligned-spf=fail"
        });
    } else {
        s.push_str("aligned-spf=absent");
    }
    s.push_str("; ");
    if dkim_pass {
        s.push_str("aligned-dkim=pass");
    } else if input.dkim.iter().any(|d| d.pass) {
        s.push_str("aligned-dkim=misaligned");
    } else if input.dkim.is_empty() {
        s.push_str("aligned-dkim=absent");
    } else {
        s.push_str("aligned-dkim=fail");
    }
    s.push_str(&format!(
        "; p={}, sp={}, adkim={}, aspf={}, pct={}",
        policy.policy, policy.subdomain_policy, policy.adkim, policy.aspf, policy.pct
    ));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Alignment;

    fn policy_with(p: PolicyAction) -> DmarcPolicy {
        DmarcPolicy {
            policy: p,
            subdomain_policy: p,
            ..DmarcPolicy::default()
        }
    }

    fn input_from(from: &str, policy_domain: &str) -> DmarcInput {
        DmarcInput {
            from_domain: from.into(),
            policy_domain: policy_domain.into(),
            spf: None,
            dkim: vec![],
        }
    }

    #[test]
    fn pass_via_aligned_spf_only() {
        let mut input = input_from("example.com", "example.com");
        input.spf = Some(SpfResult {
            domain: "example.com".into(),
            pass: true,
        });
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(out.aligned_spf_pass);
        assert!(!out.aligned_dkim_pass);
        assert!(out.dmarc_pass);
        assert_eq!(out.disposition, PolicyAction::None);
    }

    #[test]
    fn pass_via_aligned_dkim_only() {
        let mut input = input_from("example.com", "example.com");
        input.dkim = vec![DkimSignatureResult {
            d_domain: "example.com".into(),
            pass: true,
        }];
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(!out.aligned_spf_pass);
        assert!(out.aligned_dkim_pass);
        assert!(out.dmarc_pass);
    }

    #[test]
    fn fail_when_spf_misaligned() {
        let mut input = input_from("example.com", "example.com");
        input.spf = Some(SpfResult {
            domain: "different.com".into(),
            pass: true,
        });
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(!out.aligned_spf_pass);
        assert!(!out.dmarc_pass);
        assert_eq!(out.disposition, PolicyAction::Reject);
    }

    #[test]
    fn fail_when_dkim_misaligned() {
        let mut input = input_from("example.com", "example.com");
        input.dkim = vec![DkimSignatureResult {
            d_domain: "attacker.com".into(),
            pass: true,
        }];
        let out = evaluate(&policy_with(PolicyAction::Quarantine), &input);
        assert!(!out.aligned_dkim_pass);
        assert!(!out.dmarc_pass);
        assert_eq!(out.disposition, PolicyAction::Quarantine);
    }

    #[test]
    fn fail_when_spf_fail_but_aligned() {
        let mut input = input_from("example.com", "example.com");
        input.spf = Some(SpfResult {
            domain: "example.com".into(),
            pass: false,
        });
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(!out.aligned_spf_pass);
        assert!(!out.dmarc_pass);
    }

    #[test]
    fn relaxed_alignment_subdomain_passes() {
        let mut input = input_from("example.com", "example.com");
        input.spf = Some(SpfResult {
            domain: "mail.example.com".into(),
            pass: true,
        });
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(out.aligned_spf_pass);
    }

    #[test]
    fn strict_alignment_subdomain_fails() {
        let p = DmarcPolicy {
            policy: PolicyAction::Reject,
            subdomain_policy: PolicyAction::Reject,
            aspf: Alignment::Strict,
            adkim: Alignment::Strict,
            ..DmarcPolicy::default()
        };
        let mut input = input_from("example.com", "example.com");
        input.spf = Some(SpfResult {
            domain: "mail.example.com".into(),
            pass: true,
        });
        let out = evaluate(&p, &input);
        assert!(!out.aligned_spf_pass);
        assert_eq!(out.disposition, PolicyAction::Reject);
    }

    #[test]
    fn subdomain_uses_sp_policy() {
        let p = DmarcPolicy {
            policy: PolicyAction::Reject,
            subdomain_policy: PolicyAction::Quarantine,
            ..DmarcPolicy::default()
        };
        let input = input_from("sub.example.com", "example.com");
        let out = evaluate(&p, &input);
        assert!(!out.dmarc_pass);
        assert_eq!(out.disposition, PolicyAction::Quarantine);
    }

    #[test]
    fn dkim_pass_wins_even_when_spf_fails() {
        let mut input = input_from("example.com", "example.com");
        input.spf = Some(SpfResult {
            domain: "wrong.com".into(),
            pass: true,
        });
        input.dkim = vec![DkimSignatureResult {
            d_domain: "mail.example.com".into(),
            pass: true,
        }];
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(!out.aligned_spf_pass);
        assert!(out.aligned_dkim_pass);
        assert!(out.dmarc_pass);
    }

    #[test]
    fn first_passing_aligned_dkim_signature_wins() {
        let mut input = input_from("example.com", "example.com");
        input.dkim = vec![
            // First sig: wrong domain
            DkimSignatureResult {
                d_domain: "attacker.com".into(),
                pass: true,
            },
            // Second sig: correct domain, passes
            DkimSignatureResult {
                d_domain: "example.com".into(),
                pass: true,
            },
        ];
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(out.aligned_dkim_pass);
    }

    #[test]
    fn dkim_signatures_that_dont_pass_dont_count() {
        let mut input = input_from("example.com", "example.com");
        input.dkim = vec![DkimSignatureResult {
            d_domain: "example.com".into(),
            pass: false,
        }];
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(!out.aligned_dkim_pass);
    }

    #[test]
    fn no_auth_data_fails_dmarc() {
        let input = input_from("example.com", "example.com");
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(!out.dmarc_pass);
        assert_eq!(out.disposition, PolicyAction::Reject);
    }

    #[test]
    fn reason_string_captures_state() {
        let mut input = input_from("example.com", "example.com");
        input.spf = Some(SpfResult {
            domain: "example.com".into(),
            pass: true,
        });
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(out.reason.contains("aligned-spf=pass"));
        assert!(out.reason.contains("aligned-dkim=absent"));
        assert!(out.reason.contains("p=reject"));
    }

    #[test]
    fn pct_passes_through() {
        let p = DmarcPolicy {
            policy: PolicyAction::Reject,
            pct: 25,
            ..DmarcPolicy::default()
        };
        let input = input_from("example.com", "example.com");
        let out = evaluate(&p, &input);
        assert_eq!(out.pct, 25);
    }

    #[test]
    fn relaxed_default_co_uk_subdomain_aligns() {
        // From: news@example.co.uk
        // SPF MAIL FROM: bounces@mail.example.co.uk
        let mut input = input_from("example.co.uk", "example.co.uk");
        input.spf = Some(SpfResult {
            domain: "mail.example.co.uk".into(),
            pass: true,
        });
        let out = evaluate(&policy_with(PolicyAction::Reject), &input);
        assert!(out.aligned_spf_pass);
    }
}

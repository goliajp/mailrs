//! DMARC policy record parser (RFC 7489 §6.3).
//!
//! The DMARC TXT record at `_dmarc.<domain>` is a `;`-delimited tag-list
//! mirroring DKIM's syntax. This module turns that string into a
//! structured [`DmarcPolicy`] value the evaluator can act on.

use std::fmt;

/// Required first tag of every DMARC record.
const V_DMARC1: &str = "DMARC1";

/// Disposition policy declared by the domain owner (`p=` and `sp=`).
///
/// Per RFC 7489 §6.3, an unknown value is treated as `None` (most
/// lenient), but we surface the parse error so callers can decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PolicyAction {
    /// `p=none` — monitor only, deliver as if no policy were in place.
    #[default]
    None,
    /// `p=quarantine` — deliver but treat as suspicious (typically spam).
    Quarantine,
    /// `p=reject` — refuse delivery (SMTP-time 550).
    Reject,
}

impl fmt::Display for PolicyAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PolicyAction::None => "none",
            PolicyAction::Quarantine => "quarantine",
            PolicyAction::Reject => "reject",
        })
    }
}

/// Identifier alignment mode (`adkim=` / `aspf=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    /// `r` — relaxed: organizational domains must match (e.g.
    /// `mail.example.com` aligns with `example.com`).
    #[default]
    Relaxed,
    /// `s` — strict: full domain names must match exactly.
    Strict,
}

impl fmt::Display for Alignment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Alignment::Relaxed => "r",
            Alignment::Strict => "s",
        })
    }
}

/// Parsed DMARC policy record (RFC 7489 §6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmarcPolicy {
    /// `p=` — disposition for the primary domain.
    pub policy: PolicyAction,
    /// `sp=` — disposition for subdomains. Defaults to `policy` when absent.
    pub subdomain_policy: PolicyAction,
    /// `adkim=` — DKIM identifier alignment mode.
    pub adkim: Alignment,
    /// `aspf=` — SPF identifier alignment mode.
    pub aspf: Alignment,
    /// `pct=` — percentage of mail subject to filtering (0-100). 100 by default.
    pub pct: u8,
    /// `rua=` — aggregate report URIs (typically `mailto:` URIs).
    pub rua: Vec<String>,
    /// `ruf=` — forensic report URIs.
    pub ruf: Vec<String>,
}

impl Default for DmarcPolicy {
    /// RFC 7489 §6.3 defaults: `p=none`, `sp` inherits `p`, relaxed alignment, pct=100.
    fn default() -> Self {
        Self {
            policy: PolicyAction::None,
            subdomain_policy: PolicyAction::None,
            adkim: Alignment::Relaxed,
            aspf: Alignment::Relaxed,
            pct: 100,
            rua: Vec::new(),
            ruf: Vec::new(),
        }
    }
}

/// Errors returned by [`DmarcPolicy::parse`].
#[derive(Debug, thiserror::Error)]
pub enum DmarcParseError {
    /// Record didn't start with `v=DMARC1` (RFC 7489 §6.4).
    #[error("missing v=DMARC1 prefix")]
    NotADmarcRecord,
    /// Required `p=` tag missing.
    #[error("missing required p= tag")]
    MissingPolicy,
    /// `pct=` value not in 0..=100.
    #[error("pct out of range: {0}")]
    PctOutOfRange(u32),
    /// A required tag is malformed.
    #[error("malformed tag {name}: {value}")]
    MalformedTag {
        /// Tag name (e.g. "p", "pct").
        name: String,
        /// The raw value we couldn't parse.
        value: String,
    },
}

impl DmarcPolicy {
    /// Parse a DMARC TXT record string.
    ///
    /// The record must start with `v=DMARC1`. Unknown tags are ignored
    /// per RFC 7489 §6.6.3. Whitespace around `=` and between tags is
    /// tolerated.
    ///
    /// # Example
    ///
    /// ```
    /// use mailrs_dmarc::policy::{DmarcPolicy, PolicyAction, Alignment};
    ///
    /// let p = DmarcPolicy::parse(
    ///     "v=DMARC1; p=reject; sp=quarantine; adkim=s; aspf=r; pct=50; rua=mailto:agg@example.com",
    /// )
    /// .unwrap();
    /// assert_eq!(p.policy, PolicyAction::Reject);
    /// assert_eq!(p.subdomain_policy, PolicyAction::Quarantine);
    /// assert_eq!(p.adkim, Alignment::Strict);
    /// assert_eq!(p.aspf, Alignment::Relaxed);
    /// assert_eq!(p.pct, 50);
    /// assert_eq!(p.rua, vec!["mailto:agg@example.com".to_string()]);
    /// ```
    pub fn parse(record: &str) -> Result<Self, DmarcParseError> {
        let mut policy = DmarcPolicy::default();
        let mut explicit_subdomain_policy = false;
        let mut saw_version = false;
        let mut saw_policy = false;

        for raw_tag in record.split(';') {
            let tag = raw_tag.trim();
            if tag.is_empty() {
                continue;
            }
            let Some((name, value)) = tag.split_once('=') else {
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();

            match name.as_str() {
                "v" => {
                    if !value.eq_ignore_ascii_case(V_DMARC1) {
                        return Err(DmarcParseError::NotADmarcRecord);
                    }
                    saw_version = true;
                }
                "p" => {
                    policy.policy = parse_policy_action(value).ok_or_else(|| {
                        DmarcParseError::MalformedTag {
                            name: "p".into(),
                            value: value.into(),
                        }
                    })?;
                    if !explicit_subdomain_policy {
                        policy.subdomain_policy = policy.policy;
                    }
                    saw_policy = true;
                }
                "sp" => {
                    policy.subdomain_policy = parse_policy_action(value).ok_or_else(|| {
                        DmarcParseError::MalformedTag {
                            name: "sp".into(),
                            value: value.into(),
                        }
                    })?;
                    explicit_subdomain_policy = true;
                }
                "adkim" => {
                    policy.adkim =
                        parse_alignment(value).ok_or_else(|| DmarcParseError::MalformedTag {
                            name: "adkim".into(),
                            value: value.into(),
                        })?;
                }
                "aspf" => {
                    policy.aspf =
                        parse_alignment(value).ok_or_else(|| DmarcParseError::MalformedTag {
                            name: "aspf".into(),
                            value: value.into(),
                        })?;
                }
                "pct" => {
                    let n: u32 = value.parse().map_err(|_| DmarcParseError::MalformedTag {
                        name: "pct".into(),
                        value: value.into(),
                    })?;
                    if n > 100 {
                        return Err(DmarcParseError::PctOutOfRange(n));
                    }
                    policy.pct = n as u8;
                }
                "rua" => {
                    for uri in value.split(',') {
                        let uri = uri.trim();
                        if !uri.is_empty() {
                            policy.rua.push(uri.to_string());
                        }
                    }
                }
                "ruf" => {
                    for uri in value.split(',') {
                        let uri = uri.trim();
                        if !uri.is_empty() {
                            policy.ruf.push(uri.to_string());
                        }
                    }
                }
                // RFC 7489 §6.6.3: unknown tags are skipped (forward-compat).
                _ => {}
            }
        }

        if !saw_version {
            return Err(DmarcParseError::NotADmarcRecord);
        }
        if !saw_policy {
            return Err(DmarcParseError::MissingPolicy);
        }
        Ok(policy)
    }
}

fn parse_policy_action(s: &str) -> Option<PolicyAction> {
    match s.to_ascii_lowercase().as_str() {
        "none" => Some(PolicyAction::None),
        "quarantine" => Some(PolicyAction::Quarantine),
        "reject" => Some(PolicyAction::Reject),
        _ => None,
    }
}

fn parse_alignment(s: &str) -> Option<Alignment> {
    match s.to_ascii_lowercase().as_str() {
        "s" | "strict" => Some(Alignment::Strict),
        "r" | "relaxed" => Some(Alignment::Relaxed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal() {
        let p = DmarcPolicy::parse("v=DMARC1; p=none").unwrap();
        assert_eq!(p.policy, PolicyAction::None);
        assert_eq!(p.subdomain_policy, PolicyAction::None);
        assert_eq!(p.pct, 100);
    }

    #[test]
    fn parses_full() {
        let p = DmarcPolicy::parse(
            "v=DMARC1; p=reject; sp=quarantine; adkim=s; aspf=r; pct=50; \
             rua=mailto:a@x.com,mailto:b@x.com; ruf=mailto:f@x.com",
        )
        .unwrap();
        assert_eq!(p.policy, PolicyAction::Reject);
        assert_eq!(p.subdomain_policy, PolicyAction::Quarantine);
        assert_eq!(p.adkim, Alignment::Strict);
        assert_eq!(p.aspf, Alignment::Relaxed);
        assert_eq!(p.pct, 50);
        assert_eq!(p.rua.len(), 2);
        assert_eq!(p.ruf.len(), 1);
    }

    #[test]
    fn subdomain_policy_inherits_from_p() {
        let p = DmarcPolicy::parse("v=DMARC1; p=reject").unwrap();
        assert_eq!(p.subdomain_policy, PolicyAction::Reject);
    }

    #[test]
    fn subdomain_policy_explicit_wins() {
        let p = DmarcPolicy::parse("v=DMARC1; p=reject; sp=none").unwrap();
        assert_eq!(p.subdomain_policy, PolicyAction::None);
    }

    #[test]
    fn case_insensitive_tags_and_values() {
        let p = DmarcPolicy::parse("V=DMARC1; P=Reject; ADKIM=S; ASPF=Relaxed").unwrap();
        assert_eq!(p.policy, PolicyAction::Reject);
        assert_eq!(p.adkim, Alignment::Strict);
        assert_eq!(p.aspf, Alignment::Relaxed);
    }

    #[test]
    fn unknown_tags_ignored() {
        let p = DmarcPolicy::parse("v=DMARC1; p=none; futuretag=hello; another=42").unwrap();
        assert_eq!(p.policy, PolicyAction::None);
    }

    #[test]
    fn rejects_missing_version() {
        let r = DmarcPolicy::parse("p=none");
        assert!(matches!(r, Err(DmarcParseError::NotADmarcRecord)));
    }

    #[test]
    fn rejects_wrong_version() {
        let r = DmarcPolicy::parse("v=SPF1; p=none");
        assert!(matches!(r, Err(DmarcParseError::NotADmarcRecord)));
    }

    #[test]
    fn rejects_missing_policy() {
        let r = DmarcPolicy::parse("v=DMARC1");
        assert!(matches!(r, Err(DmarcParseError::MissingPolicy)));
    }

    #[test]
    fn rejects_bad_policy_value() {
        let r = DmarcPolicy::parse("v=DMARC1; p=garbage");
        assert!(matches!(r, Err(DmarcParseError::MalformedTag { .. })));
    }

    #[test]
    fn rejects_pct_over_100() {
        let r = DmarcPolicy::parse("v=DMARC1; p=none; pct=150");
        assert!(matches!(r, Err(DmarcParseError::PctOutOfRange(150))));
    }

    #[test]
    fn whitespace_tolerated() {
        let p = DmarcPolicy::parse("  v = DMARC1 ;  p = quarantine ;  pct = 25  ").unwrap();
        assert_eq!(p.policy, PolicyAction::Quarantine);
        assert_eq!(p.pct, 25);
    }

    #[test]
    fn empty_rua_list_handled() {
        let p = DmarcPolicy::parse("v=DMARC1; p=none; rua=").unwrap();
        assert!(p.rua.is_empty());
    }

    #[test]
    fn comma_separated_rua_split() {
        let p =
            DmarcPolicy::parse("v=DMARC1; p=none; rua=mailto:a@x,mailto:b@y,mailto:c@z").unwrap();
        assert_eq!(p.rua.len(), 3);
    }

    #[test]
    fn display_for_policy_action() {
        assert_eq!(PolicyAction::None.to_string(), "none");
        assert_eq!(PolicyAction::Quarantine.to_string(), "quarantine");
        assert_eq!(PolicyAction::Reject.to_string(), "reject");
    }
}

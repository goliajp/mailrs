//! The judgement half of the domain check: lookup results in, the report
//! the domain page renders out. No I/O, so every verdict is testable.

use serde::Serialize;

/// What one DNS question came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Found(Vec<String>),
    /// NXDOMAIN or no records of the asked type.
    Missing,
    /// The resolver could not answer; says nothing about the record.
    Failed(String),
    /// Not asked, with the reason.
    NotAsked(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckResult {
    pub name: &'static str,
    pub status: CheckStatus,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DomainCheckReport {
    pub domain: String,
    pub checked_at: i64,
    pub checks: Vec<CheckResult>,
}

pub struct Lookups {
    pub mx: Lookup,
    pub spf: Lookup,
    /// The selector asked for, when one is configured.
    pub dkim_selector: Option<String>,
    pub dkim: Lookup,
    pub dmarc: Lookup,
}

fn check(
    name: &'static str,
    status: CheckStatus,
    message: String,
    details: Vec<String>,
) -> CheckResult {
    CheckResult {
        name,
        status,
        message,
        details,
    }
}

/// `Missing`, `Failed` and `NotAsked` judged the same way for every record;
/// `Found` is left to the caller.
fn unresolved(name: &'static str, what: &str, lookup: &Lookup) -> Option<CheckResult> {
    match lookup {
        Lookup::Found(v) if !v.is_empty() => None,
        Lookup::Found(_) | Lookup::Missing => Some(check(
            name,
            CheckStatus::Fail,
            format!("No {what}"),
            Vec::new(),
        )),
        Lookup::Failed(e) => Some(check(
            name,
            CheckStatus::Warn,
            format!("Lookup failed for {what}"),
            vec![e.clone()],
        )),
        Lookup::NotAsked(why) => Some(check(name, CheckStatus::Warn, why.clone(), Vec::new())),
    }
}

fn found(lookup: &Lookup) -> &[String] {
    match lookup {
        Lookup::Found(v) => v,
        _ => &[],
    }
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// The value of tag `name` in a `k=v; k=v` record (DKIM, DMARC).
fn tag<'a>(record: &'a str, name: &str) -> Option<&'a str> {
    record.split(';').find_map(|part| {
        let (k, v) = part.split_once('=')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn mx_check(lookup: &Lookup) -> CheckResult {
    if let Some(r) = unresolved("MX", "MX record", lookup) {
        return r;
    }
    let hosts = found(lookup).to_vec();
    check(
        "MX",
        CheckStatus::Pass,
        format!("{} MX host(s)", hosts.len()),
        hosts,
    )
}

fn spf_check(lookup: &Lookup) -> CheckResult {
    if let Some(r) = unresolved("SPF", "SPF record", lookup) {
        return r;
    }
    let spf: Vec<String> = found(lookup)
        .iter()
        .filter(|t| starts_with_ci(t, "v=spf1 ") || t.eq_ignore_ascii_case("v=spf1"))
        .cloned()
        .collect();
    match spf.as_slice() {
        [] => check("SPF", CheckStatus::Fail, "No SPF record".into(), Vec::new()),
        [one] => {
            let last = one
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_ascii_lowercase();
            let redirect = one
                .split_whitespace()
                .any(|t| starts_with_ci(t, "redirect="));
            let (status, message) = match last.as_str() {
                "-all" => (CheckStatus::Pass, "Other senders are rejected (-all)"),
                "~all" => (CheckStatus::Pass, "Other senders soft-fail (~all)"),
                _ if redirect => (CheckStatus::Pass, "Policy delegated with redirect="),
                _ => (CheckStatus::Warn, "Record does not end in -all or ~all"),
            };
            check("SPF", status, message.into(), spf)
        }
        _ => check(
            "SPF",
            CheckStatus::Fail,
            "More than one SPF record; receivers treat this as an error".into(),
            spf,
        ),
    }
}

fn dkim_check(selector: Option<&str>, domain: &str, lookup: &Lookup) -> CheckResult {
    let what = match selector {
        Some(s) => format!("DKIM key at {s}._domainkey.{domain}"),
        None => "DKIM key".to_string(),
    };
    if let Some(r) = unresolved("DKIM", &what, lookup) {
        return r;
    }
    let keys: Vec<String> = found(lookup)
        .iter()
        .filter(|t| tag(t, "p").is_some())
        .cloned()
        .collect();
    if keys.is_empty() {
        return check(
            "DKIM",
            CheckStatus::Fail,
            format!("No {what}"),
            found(lookup).to_vec(),
        );
    }
    if keys
        .iter()
        .any(|k| tag(k, "p").is_some_and(|p| !p.is_empty()))
    {
        return check(
            "DKIM",
            CheckStatus::Pass,
            format!("{what} is published"),
            keys,
        );
    }
    check(
        "DKIM",
        CheckStatus::Fail,
        format!("{what} is revoked (empty p=)"),
        keys,
    )
}

fn dmarc_check(lookup: &Lookup) -> CheckResult {
    if let Some(r) = unresolved("DMARC", "DMARC record", lookup) {
        return r;
    }
    let records: Vec<String> = found(lookup)
        .iter()
        .filter(|t| starts_with_ci(t, "v=DMARC1"))
        .cloned()
        .collect();
    let [one] = records.as_slice() else {
        let message = match records.len() {
            0 => "No DMARC record",
            _ => "More than one DMARC record; receivers ignore all of them",
        };
        return check("DMARC", CheckStatus::Fail, message.into(), records);
    };
    let policy = tag(one, "p").map(str::to_ascii_lowercase);
    let (status, message) = match policy.as_deref() {
        Some("reject") => (CheckStatus::Pass, "Policy is reject".to_string()),
        Some("quarantine") => (CheckStatus::Pass, "Policy is quarantine".to_string()),
        Some("none") => (
            CheckStatus::Warn,
            "Policy is none: mail that fails is still delivered".to_string(),
        ),
        _ => (CheckStatus::Fail, "Record has no valid p= tag".to_string()),
    };
    check("DMARC", status, message, records)
}

/// Judge the lookups. Pure: same input, same report.
pub fn build_report(domain: &str, checked_at: i64, lookups: &Lookups) -> DomainCheckReport {
    DomainCheckReport {
        domain: domain.to_string(),
        checked_at,
        checks: vec![
            mx_check(&lookups.mx),
            spf_check(&lookups.spf),
            dkim_check(lookups.dkim_selector.as_deref(), domain, &lookups.dkim),
            dmarc_check(&lookups.dmarc),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(v: &[&str]) -> Lookup {
        Lookup::Found(v.iter().map(|s| s.to_string()).collect())
    }

    fn healthy() -> Lookups {
        Lookups {
            mx: found(&["10 mail.golia.ai"]),
            spf: found(&["google-site-verification=x", "v=spf1 mx -all"]),
            dkim_selector: Some("mail".into()),
            dkim: found(&["v=DKIM1; k=rsa; p=MIIBIjANBg"]),
            dmarc: found(&["v=DMARC1; p=reject; rua=mailto:dmarc@golia.jp"]),
        }
    }

    fn statuses(r: &DomainCheckReport) -> Vec<(&'static str, CheckStatus)> {
        r.checks.iter().map(|c| (c.name, c.status)).collect()
    }

    #[test]
    fn a_healthy_domain_passes_all_four() {
        let r = build_report("golia.jp", 1_792_000_000, &healthy());
        assert_eq!(r.domain, "golia.jp");
        assert_eq!(r.checked_at, 1_792_000_000);
        assert_eq!(
            statuses(&r),
            vec![
                ("MX", CheckStatus::Pass),
                ("SPF", CheckStatus::Pass),
                ("DKIM", CheckStatus::Pass),
                ("DMARC", CheckStatus::Pass),
            ]
        );
        // only the SPF record is reported, not the other TXT at the apex
        assert_eq!(r.checks[1].details, vec!["v=spf1 mx -all".to_string()]);
        assert_eq!(
            r.checks[2].message,
            "DKIM key at mail._domainkey.golia.jp is published"
        );
    }

    #[test]
    fn missing_records_fail_and_resolver_errors_warn() {
        let l = Lookups {
            mx: Lookup::Missing,
            spf: found(&["google-site-verification=x"]),
            dkim_selector: Some("mail".into()),
            dkim: Lookup::Failed("timed out".into()),
            dmarc: Lookup::Missing,
        };
        let r = build_report("example.com", 0, &l);
        assert_eq!(
            statuses(&r),
            vec![
                ("MX", CheckStatus::Fail),
                ("SPF", CheckStatus::Fail),
                ("DKIM", CheckStatus::Warn),
                ("DMARC", CheckStatus::Fail),
            ]
        );
        assert_eq!(r.checks[2].details, vec!["timed out".to_string()]);
        assert_eq!(r.checks[0].message, "No MX record");
        assert_eq!(
            r.checks[2].message,
            "Lookup failed for DKIM key at mail._domainkey.example.com"
        );
    }

    #[test]
    fn weak_policies_warn() {
        let mut l = healthy();
        l.spf = found(&["v=spf1 mx ?all"]);
        l.dmarc = found(&["v=DMARC1; p=none"]);
        let r = build_report("golia.jp", 0, &l);
        assert_eq!(r.checks[1].status, CheckStatus::Warn);
        assert_eq!(r.checks[3].status, CheckStatus::Warn);
        l.spf = found(&["v=spf1 redirect=_spf.golia.jp"]);
        assert_eq!(
            build_report("golia.jp", 0, &l).checks[1].status,
            CheckStatus::Pass
        );
    }

    #[test]
    fn duplicate_or_revoked_records_fail() {
        let mut l = healthy();
        l.spf = found(&["v=spf1 -all", "v=spf1 mx -all"]);
        l.dkim = found(&["v=DKIM1; p="]);
        l.dmarc = found(&["v=DMARC1; p=reject", "v=DMARC1; p=none"]);
        let r = build_report("golia.jp", 0, &l);
        assert_eq!(r.checks[1].status, CheckStatus::Fail);
        assert_eq!(r.checks[2].status, CheckStatus::Fail);
        assert!(r.checks[2].message.contains("revoked"));
        assert_eq!(r.checks[3].status, CheckStatus::Fail);
    }

    #[test]
    fn no_selector_means_dkim_was_not_checked() {
        let mut l = healthy();
        l.dkim_selector = None;
        l.dkim = Lookup::NotAsked("MAILRS_DKIM_SELECTOR is not set on webapi".into());
        let c = &build_report("golia.jp", 0, &l).checks[2];
        assert_eq!(c.status, CheckStatus::Warn);
        assert_eq!(c.message, "MAILRS_DKIM_SELECTOR is not set on webapi");
    }

    #[test]
    fn serializes_to_the_shape_the_page_reads() {
        let v = serde_json::to_value(build_report("golia.jp", 5, &healthy())).unwrap();
        assert_eq!(v["checked_at"], 5);
        assert_eq!(v["checks"][0]["name"], "MX");
        assert_eq!(v["checks"][0]["status"], "pass");
        assert!(v["checks"][0]["message"].is_string());
        assert!(v["checks"][0]["details"].is_array());
    }
}

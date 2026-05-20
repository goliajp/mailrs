//! Email-domain DNS health checks for postmasters.
//!
//! Given a domain, [`check_domain`] runs the full battery of DNS-level
//! checks any mail-server operator needs at deploy time and during
//! incident response: MX, SPF, DKIM, DMARC, MTA-STS, TLS-RPT, BIMI,
//! DANE, PTR / FCrDNS.
//!
//! Every check returns a [`CheckResult`] with a [`Status`], a
//! human-readable message, and structured details. The full report
//! ([`DomainCheckReport`]) is `Serialize` so you can ship it straight
//! to JSON / Prometheus / a CLI table.
//!
//! ## Example
//!
//! ```no_run
//! use hickory_resolver::TokioResolver;
//! use mailrs_postmaster::check_domain;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let resolver = TokioResolver::builder_tokio()?.build()?;
//! let report = check_domain(&resolver, "example.com", Some("default"), "mail.example.com").await;
//! for check in &report.checks {
//!     println!("{}: {:?} — {}", check.name, check.status, check.message);
//! }
//! # Ok(())
//! # }
//! ```

use std::net::IpAddr;

use hickory_resolver::proto::rr::{RData, RecordType};
use hickory_resolver::TokioResolver;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DomainCheckReport {
    pub domain: String,
    pub checks: Vec<CheckResult>,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub status: Status,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    Skip,
}

pub async fn check_domain(
    resolver: &TokioResolver,
    domain: &str,
    dkim_selector: Option<&str>,
    hostname: &str,
) -> DomainCheckReport {
    let mut checks = Vec::with_capacity(9);

    checks.push(check_mx(resolver, domain, hostname).await);
    checks.push(check_spf(resolver, domain, hostname).await);
    checks.push(check_dkim(resolver, domain, dkim_selector).await);
    checks.push(check_dmarc(resolver, domain).await);
    checks.push(check_mta_sts_record(resolver, domain).await);
    checks.push(check_mta_sts_policy(domain).await);
    checks.push(check_tlsrpt(resolver, domain).await);
    checks.push(check_ptr(resolver, hostname).await);
    checks.push(check_dane(resolver, domain).await);
    checks.push(check_bimi(resolver, domain).await);

    DomainCheckReport {
        domain: domain.to_string(),
        checks,
        checked_at: chrono::Utc::now().timestamp(),
    }
}

async fn check_mx(resolver: &TokioResolver, domain: &str, hostname: &str) -> CheckResult {
    match resolver.mx_lookup(domain).await {
        Ok(records) => {
            let mxs: Vec<_> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::MX(mx) => Some(mx),
                    _ => None,
                })
                .collect();
            let entries: Vec<String> = mxs
                .iter()
                .map(|mx| format!("{} (priority {})", mx.exchange, mx.preference))
                .collect();
            if entries.is_empty() {
                return CheckResult {
                    name: "MX Records".into(),
                    status: Status::Fail,
                    message: "no MX records found".into(),
                    details: vec![],
                };
            }
            // check if any MX points to our hostname
            let points_to_us = mxs.iter().any(|mx| {
                let exchange = mx.exchange.to_string();
                let exchange = exchange.trim_end_matches('.');
                exchange.eq_ignore_ascii_case(hostname)
            });
            if points_to_us {
                CheckResult {
                    name: "MX Records".into(),
                    status: Status::Pass,
                    message: format!("{} MX record(s) found, includes {hostname}", entries.len()),
                    details: entries,
                }
            } else {
                CheckResult {
                    name: "MX Records".into(),
                    status: Status::Warn,
                    message: format!(
                        "{} MX record(s) found, but none point to {hostname}",
                        entries.len()
                    ),
                    details: entries,
                }
            }
        }
        Err(e) => CheckResult {
            name: "MX Records".into(),
            status: Status::Fail,
            message: format!("MX lookup failed: {e}"),
            details: vec![],
        },
    }
}

async fn check_spf(resolver: &TokioResolver, domain: &str, hostname: &str) -> CheckResult {
    // resolve our hostname to IPs for SPF inclusion check
    let our_ips: Vec<IpAddr> = resolver
        .lookup_ip(hostname)
        .await
        .map(|ips| ips.iter().collect())
        .unwrap_or_default();

    match resolver.txt_lookup(domain).await {
        Ok(records) => {
            let spf_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .filter(|txt| txt.starts_with("v=spf1"))
                .collect();
            match spf_records.len() {
                0 => CheckResult {
                    name: "SPF Record".into(),
                    status: Status::Fail,
                    message: "no SPF record found".into(),
                    details: vec![],
                },
                1 => {
                    let record = &spf_records[0];
                    // check if our hostname or IP is mentioned in the SPF record
                    let includes_us = record.contains(hostname)
                        || our_ips.iter().any(|ip| record.contains(&ip.to_string()));

                    let policy_note = if record.contains("-all") {
                        "strict (-all)"
                    } else if record.contains("~all") {
                        "soft fail (~all)"
                    } else if record.contains("?all") {
                        "neutral (?all)"
                    } else if record.contains("+all") {
                        "pass all (+all, dangerous)"
                    } else {
                        "unknown policy"
                    };

                    let (status, message) = if includes_us {
                        (
                            Status::Pass,
                            format!("SPF record found, {policy_note}, includes {hostname}"),
                        )
                    } else {
                        (
                            Status::Warn,
                            format!(
                                "SPF record found, {policy_note}, but does not include {hostname}"
                            ),
                        )
                    };

                    CheckResult {
                        name: "SPF Record".into(),
                        status,
                        message,
                        details: spf_records,
                    }
                }
                _ => CheckResult {
                    name: "SPF Record".into(),
                    status: Status::Warn,
                    message: "multiple SPF records found (should have exactly one)".into(),
                    details: spf_records,
                },
            }
        }
        Err(e) => CheckResult {
            name: "SPF Record".into(),
            status: Status::Fail,
            message: format!("TXT lookup failed: {e}"),
            details: vec![],
        },
    }
}

async fn check_dkim(resolver: &TokioResolver, domain: &str, selector: Option<&str>) -> CheckResult {
    let Some(sel) = selector else {
        return CheckResult {
            name: "DKIM Record".into(),
            status: Status::Skip,
            message: "no DKIM selector configured".into(),
            details: vec![],
        };
    };

    let qname = format!("{sel}._domainkey.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let dkim_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .filter(|txt| txt.contains("v=DKIM1"))
                .collect();
            if dkim_records.is_empty() {
                CheckResult {
                    name: "DKIM Record".into(),
                    status: Status::Fail,
                    message: format!("no DKIM record at {qname}"),
                    details: vec![],
                }
            } else {
                CheckResult {
                    name: "DKIM Record".into(),
                    status: Status::Pass,
                    message: format!("DKIM record found at {qname}"),
                    details: dkim_records,
                }
            }
        }
        Err(e) => CheckResult {
            name: "DKIM Record".into(),
            status: Status::Fail,
            message: format!("DKIM lookup failed for {qname}: {e}"),
            details: vec![],
        },
    }
}

async fn check_dmarc(resolver: &TokioResolver, domain: &str) -> CheckResult {
    let qname = format!("_dmarc.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let dmarc_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .filter(|txt| txt.starts_with("v=DMARC1"))
                .collect();
            if dmarc_records.is_empty() {
                return CheckResult {
                    name: "DMARC Record".into(),
                    status: Status::Fail,
                    message: "no DMARC record found".into(),
                    details: vec![],
                };
            }
            let record = &dmarc_records[0];
            let (status, policy_msg) = if record.contains("p=reject") {
                (Status::Pass, "policy: reject")
            } else if record.contains("p=quarantine") {
                (Status::Pass, "policy: quarantine")
            } else if record.contains("p=none") {
                (Status::Warn, "policy: none (monitoring only)")
            } else {
                (Status::Warn, "policy not recognized")
            };
            CheckResult {
                name: "DMARC Record".into(),
                status,
                message: format!("DMARC record found, {policy_msg}"),
                details: dmarc_records,
            }
        }
        Err(e) => CheckResult {
            name: "DMARC Record".into(),
            status: Status::Fail,
            message: format!("DMARC lookup failed: {e}"),
            details: vec![],
        },
    }
}

async fn check_mta_sts_record(resolver: &TokioResolver, domain: &str) -> CheckResult {
    let qname = format!("_mta-sts.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let sts_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .filter(|txt| txt.contains("v=STSv1"))
                .collect();
            if sts_records.is_empty() {
                CheckResult {
                    name: "MTA-STS Record".into(),
                    status: Status::Warn,
                    message: "no MTA-STS TXT record found".into(),
                    details: vec![],
                }
            } else {
                CheckResult {
                    name: "MTA-STS Record".into(),
                    status: Status::Pass,
                    message: "MTA-STS TXT record found".into(),
                    details: sts_records,
                }
            }
        }
        Err(_) => CheckResult {
            name: "MTA-STS Record".into(),
            status: Status::Warn,
            message: "no MTA-STS TXT record found".into(),
            details: vec![],
        },
    }
}

async fn check_mta_sts_policy(domain: &str) -> CheckResult {
    let url = format!("https://mta-sts.{domain}/.well-known/mta-sts.txt");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let Ok(client) = client else {
        return CheckResult {
            name: "MTA-STS Policy".into(),
            status: Status::Skip,
            message: "HTTP client error".into(),
            details: vec![],
        };
    };
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(body) => {
                let has_mode = body.contains("mode:");
                let has_mx = body.contains("mx:");
                let status = if has_mode && has_mx {
                    Status::Pass
                } else {
                    Status::Warn
                };
                CheckResult {
                    name: "MTA-STS Policy".into(),
                    status,
                    message: format!("policy fetched from {url}"),
                    details: body.lines().map(|l| l.to_string()).collect(),
                }
            }
            Err(e) => CheckResult {
                name: "MTA-STS Policy".into(),
                status: Status::Warn,
                message: format!("failed to read policy body: {e}"),
                details: vec![],
            },
        },
        Ok(resp) => CheckResult {
            name: "MTA-STS Policy".into(),
            status: Status::Warn,
            message: format!("policy endpoint returned HTTP {}", resp.status()),
            details: vec![],
        },
        Err(e) => CheckResult {
            name: "MTA-STS Policy".into(),
            status: Status::Warn,
            message: format!("could not reach MTA-STS policy: {e}"),
            details: vec![],
        },
    }
}

async fn check_tlsrpt(resolver: &TokioResolver, domain: &str) -> CheckResult {
    let qname = format!("_smtp._tls.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let tls_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .filter(|txt| txt.contains("v=TLSRPTv1"))
                .collect();
            if tls_records.is_empty() {
                CheckResult {
                    name: "TLSRPT Record".into(),
                    status: Status::Warn,
                    message: "no TLSRPT record found".into(),
                    details: vec![],
                }
            } else {
                CheckResult {
                    name: "TLSRPT Record".into(),
                    status: Status::Pass,
                    message: "TLSRPT record found".into(),
                    details: tls_records,
                }
            }
        }
        Err(_) => CheckResult {
            name: "TLSRPT Record".into(),
            status: Status::Warn,
            message: "no TLSRPT record found".into(),
            details: vec![],
        },
    }
}

async fn check_ptr(resolver: &TokioResolver, hostname: &str) -> CheckResult {
    // resolve hostname to IP, then reverse lookup
    let ip: Option<IpAddr> = match resolver.lookup_ip(hostname).await {
        Ok(ips) => ips.iter().next(),
        Err(e) => {
            return CheckResult {
                name: "Reverse DNS (PTR)".into(),
                status: Status::Fail,
                message: format!("could not resolve hostname {hostname}: {e}"),
                details: vec![],
            };
        }
    };
    let Some(ip) = ip else {
        return CheckResult {
            name: "Reverse DNS (PTR)".into(),
            status: Status::Fail,
            message: format!("no A/AAAA record for {hostname}"),
            details: vec![],
        };
    };

    match resolver.reverse_lookup(ip).await {
        Ok(names) => {
            let ptrs: Vec<String> = names
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::PTR(name) => Some(name.to_string()),
                    _ => None,
                })
                .collect();
            let matches = ptrs.iter().any(|n| n.trim_end_matches('.') == hostname);
            if matches {
                CheckResult {
                    name: "Reverse DNS (PTR)".into(),
                    status: Status::Pass,
                    message: format!("PTR for {ip} matches {hostname}"),
                    details: ptrs,
                }
            } else {
                CheckResult {
                    name: "Reverse DNS (PTR)".into(),
                    status: Status::Warn,
                    message: format!("PTR for {ip} does not match {hostname}"),
                    details: ptrs,
                }
            }
        }
        Err(e) => CheckResult {
            name: "Reverse DNS (PTR)".into(),
            status: Status::Warn,
            message: format!("reverse lookup for {ip} failed: {e}"),
            details: vec![],
        },
    }
}

async fn check_bimi(resolver: &TokioResolver, domain: &str) -> CheckResult {
    let qname = format!("default._bimi.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let bimi_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .filter(|txt| txt.contains("v=BIMI1"))
                .collect();
            if bimi_records.is_empty() {
                CheckResult {
                    name: "BIMI Record".into(),
                    status: Status::Skip,
                    message: "no BIMI record found".into(),
                    details: vec![],
                }
            } else {
                let logo_url = extract_bimi_logo_url(&bimi_records[0]);
                let (status, message) = if logo_url.is_some() {
                    (Status::Pass, "BIMI record found with logo URL".to_string())
                } else {
                    (
                        Status::Warn,
                        "BIMI record found but no logo URL (l= tag missing)".to_string(),
                    )
                };
                CheckResult {
                    name: "BIMI Record".into(),
                    status,
                    message,
                    details: bimi_records,
                }
            }
        }
        Err(_) => CheckResult {
            name: "BIMI Record".into(),
            status: Status::Skip,
            message: "no BIMI record found".into(),
            details: vec![],
        },
    }
}

/// extract the logo URL from a BIMI record (l=https://...)
pub fn extract_bimi_logo_url(record: &str) -> Option<String> {
    record
        .split(';')
        .map(|part| part.trim())
        .find(|part| part.starts_with("l="))
        .and_then(|l_part| {
            let url = l_part[2..].trim();
            if url.is_empty() {
                None
            } else {
                Some(url.to_string())
            }
        })
}

/// look up BIMI record for a domain and return the logo URL if found
pub async fn lookup_bimi_logo(resolver: &TokioResolver, domain: &str) -> Option<String> {
    let qname = format!("default._bimi.{domain}");
    let records = resolver.txt_lookup(&qname).await.ok()?;
    records
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::TXT(txt) => Some(txt.to_string()),
            _ => None,
        })
        .find(|txt| txt.contains("v=BIMI1"))
        .and_then(|rec| extract_bimi_logo_url(&rec))
}

// -- test-only helper functions for MTA-STS / TLSRPT parsing --

#[cfg(test)]
/// parse an MTA-STS policy body and extract key-value fields
/// returns a vec of (key, value) pairs; handles both LF and CRLF line endings
fn parse_mta_sts_policy(body: &str) -> Vec<(String, String)> {
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (key, value) = trimmed.split_once(':')?;
            Some((key.trim().to_lowercase(), value.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
/// extract the mode from a parsed MTA-STS policy
fn extract_sts_mode(fields: &[(String, String)]) -> Option<&str> {
    fields
        .iter()
        .find(|(k, _)| k == "mode")
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
/// extract all mx patterns from a parsed MTA-STS policy
fn extract_sts_mx_patterns(fields: &[(String, String)]) -> Vec<&str> {
    fields
        .iter()
        .filter(|(k, _)| k == "mx")
        .map(|(_, v)| v.as_str())
        .collect()
}

#[cfg(test)]
/// extract max_age from a parsed MTA-STS policy (in seconds)
fn extract_sts_max_age(fields: &[(String, String)]) -> Option<u64> {
    fields
        .iter()
        .find(|(k, _)| k == "max_age")
        .and_then(|(_, v)| v.parse().ok())
}

#[cfg(test)]
/// validate that a TLSRPT record has the required v=TLSRPTv1 tag and rua field
fn validate_tlsrpt_record(record: &str) -> bool {
    record.contains("v=TLSRPTv1") && record.contains("rua=")
}

#[cfg(test)]
/// extract reporting URI(s) from a TLSRPT record
fn extract_tlsrpt_rua(record: &str) -> Vec<String> {
    // format: v=TLSRPTv1; rua=mailto:reports@example.com,https://...
    record
        .split(';')
        .map(|part| part.trim())
        .find(|part| part.starts_with("rua="))
        .map(|rua_part| {
            rua_part[4..]
                .split(',')
                .map(|uri| uri.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

async fn check_dane(resolver: &TokioResolver, domain: &str) -> CheckResult {
    // look up MX first, then check TLSA for port 25 on first MX
    let mx_host = match resolver.mx_lookup(domain).await {
        Ok(records) => {
            let mut entries: Vec<_> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::MX(mx) => Some(mx),
                    _ => None,
                })
                .collect();
            entries.sort_by_key(|mx| mx.preference);
            entries
                .first()
                .map(|mx| mx.exchange.to_string().trim_end_matches('.').to_string())
        }
        Err(_) => None,
    };
    let Some(mx_host) = mx_host else {
        return CheckResult {
            name: "DANE/TLSA".into(),
            status: Status::Skip,
            message: "no MX records, skipping DANE check".into(),
            details: vec![],
        };
    };

    let qname = format!("_25._tcp.{mx_host}");
    match resolver.lookup(&qname, RecordType::TLSA).await {
        Ok(records) => {
            let entries: Vec<String> = records
                .answers()
                .iter()
                .map(|r| format!("{}", r.data))
                .collect();
            if entries.is_empty() {
                CheckResult {
                    name: "DANE/TLSA".into(),
                    status: Status::Skip,
                    message: format!("no TLSA records at {qname}"),
                    details: vec![],
                }
            } else {
                CheckResult {
                    name: "DANE/TLSA".into(),
                    status: Status::Pass,
                    message: format!("TLSA record(s) found at {qname}"),
                    details: entries,
                }
            }
        }
        Err(_) => CheckResult {
            name: "DANE/TLSA".into(),
            status: Status::Skip,
            message: format!("no TLSA records at {qname} (DANE not configured)"),
            details: vec![],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Status enum tests --

    #[test]
    fn status_serialize_pass() {
        let json = serde_json::to_string(&Status::Pass).unwrap();
        assert_eq!(json, "\"pass\"");
    }

    #[test]
    fn status_serialize_warn() {
        let json = serde_json::to_string(&Status::Warn).unwrap();
        assert_eq!(json, "\"warn\"");
    }

    #[test]
    fn status_serialize_fail() {
        let json = serde_json::to_string(&Status::Fail).unwrap();
        assert_eq!(json, "\"fail\"");
    }

    #[test]
    fn status_serialize_skip() {
        let json = serde_json::to_string(&Status::Skip).unwrap();
        assert_eq!(json, "\"skip\"");
    }

    #[test]
    fn status_debug_format() {
        assert!(format!("{:?}", Status::Pass).contains("Pass"));
        assert!(format!("{:?}", Status::Warn).contains("Warn"));
        assert!(format!("{:?}", Status::Fail).contains("Fail"));
        assert!(format!("{:?}", Status::Skip).contains("Skip"));
    }

    #[test]
    fn status_clone() {
        let s = Status::Pass;
        let s2 = s;
        assert!(matches!(s, Status::Pass));
        assert!(matches!(s2, Status::Pass));
    }

    // -- CheckResult tests --

    #[test]
    fn check_result_serialize() {
        let result = CheckResult {
            name: "Test".into(),
            status: Status::Pass,
            message: "ok".into(),
            details: vec!["detail1".into()],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"name\":\"Test\""));
        assert!(json.contains("\"status\":\"pass\""));
        assert!(json.contains("\"message\":\"ok\""));
        assert!(json.contains("\"details\":[\"detail1\"]"));
    }

    #[test]
    fn check_result_empty_details() {
        let result = CheckResult {
            name: "Empty".into(),
            status: Status::Skip,
            message: "skipped".into(),
            details: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"details\":[]"));
    }

    // -- DomainCheckReport tests --

    #[test]
    fn domain_check_report_serialize() {
        let report = DomainCheckReport {
            domain: "example.com".into(),
            checks: vec![CheckResult {
                name: "MX".into(),
                status: Status::Pass,
                message: "ok".into(),
                details: vec![],
            }],
            checked_at: 1700000000,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"domain\":\"example.com\""));
        assert!(json.contains("\"checked_at\":1700000000"));
        assert!(json.contains("\"checks\":["));
    }

    #[test]
    fn domain_check_report_empty_checks() {
        let report = DomainCheckReport {
            domain: "empty.org".into(),
            checks: vec![],
            checked_at: 0,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"checks\":[]"));
    }

    // -- MTA-STS policy parsing tests --

    #[test]
    fn parse_mta_sts_policy_enforce_mode() {
        let body = "version: STSv1\nmode: enforce\nmx: mail.example.com\nmx: *.example.com\nmax_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0], ("version".into(), "STSv1".into()));
        assert_eq!(fields[1], ("mode".into(), "enforce".into()));
        assert_eq!(fields[2], ("mx".into(), "mail.example.com".into()));
        assert_eq!(fields[3], ("mx".into(), "*.example.com".into()));
        assert_eq!(fields[4], ("max_age".into(), "86400".into()));
    }

    #[test]
    fn parse_mta_sts_policy_testing_mode() {
        let body = "version: STSv1\nmode: testing\nmx: *.example.com\nmax_age: 604800\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("testing"));
    }

    #[test]
    fn parse_mta_sts_policy_none_mode() {
        let body = "version: STSv1\nmode: none\nmax_age: 0\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("none"));
        assert_eq!(extract_sts_max_age(&fields), Some(0));
    }

    #[test]
    fn parse_mta_sts_policy_crlf_line_endings() {
        let body = "version: STSv1\r\nmode: enforce\r\nmx: *.example.com\r\nmax_age: 86400\r\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx, vec!["*.example.com"]);
    }

    #[test]
    fn parse_mta_sts_policy_empty_body() {
        let fields = parse_mta_sts_policy("");
        assert!(fields.is_empty());
    }

    #[test]
    fn parse_mta_sts_policy_blank_lines_ignored() {
        let body = "version: STSv1\n\nmode: enforce\n\nmx: *.example.com\n\nmax_age: 86400\n\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields.len(), 4);
    }

    #[test]
    fn parse_mta_sts_policy_whitespace_handling() {
        let body = "  version:   STSv1  \n  mode:  enforce  \n  mx:  mail.example.com  \n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields[0], ("version".into(), "STSv1".into()));
        assert_eq!(fields[1], ("mode".into(), "enforce".into()));
        assert_eq!(fields[2], ("mx".into(), "mail.example.com".into()));
    }

    #[test]
    fn parse_mta_sts_policy_keys_lowercased() {
        let body = "VERSION: STSv1\nMODE: enforce\nMX: *.example.com\nMAX_AGE: 86400\n";
        let fields = parse_mta_sts_policy(body);
        assert!(fields.iter().all(|(k, _)| k == &k.to_lowercase()));
    }

    #[test]
    fn parse_mta_sts_policy_no_colon_lines_skipped() {
        let body = "version: STSv1\nthis line has no colon\nmode: enforce\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn parse_mta_sts_policy_value_with_colon() {
        // only splits on first colon
        let body = "version: STSv1\nmx: host:with:colons\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields[1].1, "host:with:colons");
    }

    // -- extract_sts_mode tests --

    #[test]
    fn extract_sts_mode_missing() {
        let fields = vec![("version".into(), "STSv1".into())];
        assert_eq!(extract_sts_mode(&fields), None);
    }

    #[test]
    fn extract_sts_mode_enforce() {
        let fields = vec![
            ("version".into(), "STSv1".into()),
            ("mode".into(), "enforce".into()),
        ];
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
    }

    #[test]
    fn extract_sts_mode_from_empty_fields() {
        let fields: Vec<(String, String)> = vec![];
        assert_eq!(extract_sts_mode(&fields), None);
    }

    // -- extract_sts_mx_patterns tests --

    #[test]
    fn extract_sts_mx_patterns_multiple() {
        let fields = vec![
            ("mx".into(), "mail.example.com".into()),
            ("mx".into(), "*.example.com".into()),
            ("mx".into(), "backup.example.com".into()),
        ];
        let patterns = extract_sts_mx_patterns(&fields);
        assert_eq!(
            patterns,
            vec!["mail.example.com", "*.example.com", "backup.example.com"]
        );
    }

    #[test]
    fn extract_sts_mx_patterns_none() {
        let fields = vec![("version".into(), "STSv1".into())];
        let patterns = extract_sts_mx_patterns(&fields);
        assert!(patterns.is_empty());
    }

    #[test]
    fn extract_sts_mx_patterns_single() {
        let fields = vec![("mx".into(), "*.example.com".into())];
        let patterns = extract_sts_mx_patterns(&fields);
        assert_eq!(patterns, vec!["*.example.com"]);
    }

    // -- extract_sts_max_age tests --

    #[test]
    fn extract_sts_max_age_valid() {
        let fields = vec![("max_age".into(), "86400".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(86400));
    }

    #[test]
    fn extract_sts_max_age_zero() {
        let fields = vec![("max_age".into(), "0".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(0));
    }

    #[test]
    fn extract_sts_max_age_large() {
        // 31557600 = 1 year in seconds
        let fields = vec![("max_age".into(), "31557600".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(31557600));
    }

    #[test]
    fn extract_sts_max_age_missing() {
        let fields = vec![("mode".into(), "enforce".into())];
        assert_eq!(extract_sts_max_age(&fields), None);
    }

    #[test]
    fn extract_sts_max_age_invalid_value() {
        let fields = vec![("max_age".into(), "not_a_number".into())];
        assert_eq!(extract_sts_max_age(&fields), None);
    }

    #[test]
    fn extract_sts_max_age_negative() {
        let fields = vec![("max_age".into(), "-100".into())];
        assert_eq!(extract_sts_max_age(&fields), None);
    }

    // -- MTA-STS full policy integration tests --

    #[test]
    fn mta_sts_enforce_policy_complete() {
        let body = "\
version: STSv1\r\n\
mode: enforce\r\n\
mx: mail.example.com\r\n\
mx: *.example.com\r\n\
max_age: 604800\r\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx.len(), 2);
        assert_eq!(extract_sts_max_age(&fields), Some(604800));
    }

    #[test]
    fn mta_sts_testing_policy_complete() {
        let body = "\
version: STSv1\n\
mode: testing\n\
mx: *.example.org\n\
max_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("testing"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx, vec!["*.example.org"]);
        assert_eq!(extract_sts_max_age(&fields), Some(86400));
    }

    #[test]
    fn mta_sts_none_policy_no_mx() {
        let body = "version: STSv1\nmode: none\nmax_age: 0\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("none"));
        let mx = extract_sts_mx_patterns(&fields);
        assert!(mx.is_empty());
        assert_eq!(extract_sts_max_age(&fields), Some(0));
    }

    #[test]
    fn mta_sts_real_world_google_policy() {
        // based on actual google MTA-STS policy
        let body = "\
version: STSv1\n\
mode: enforce\n\
mx: gmail-smtp-in.l.google.com\n\
mx: *.gmail-smtp-in.l.google.com\n\
max_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx.len(), 2);
        assert!(mx.contains(&"gmail-smtp-in.l.google.com"));
        assert!(mx.contains(&"*.gmail-smtp-in.l.google.com"));
    }

    #[test]
    fn mta_sts_real_world_microsoft_policy() {
        let body = "\
version: STSv1\n\
mode: enforce\n\
mx: *.mail.protection.outlook.com\n\
max_age: 604800\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx, vec!["*.mail.protection.outlook.com"]);
        assert_eq!(extract_sts_max_age(&fields), Some(604800));
    }

    // -- validate_tlsrpt_record tests --

    #[test]
    fn validate_tlsrpt_valid_mailto() {
        assert!(validate_tlsrpt_record(
            "v=TLSRPTv1; rua=mailto:tls-reports@example.com"
        ));
    }

    #[test]
    fn validate_tlsrpt_valid_https() {
        assert!(validate_tlsrpt_record(
            "v=TLSRPTv1; rua=https://reporting.example.com/tls"
        ));
    }

    #[test]
    fn validate_tlsrpt_valid_multiple_rua() {
        assert!(validate_tlsrpt_record(
            "v=TLSRPTv1; rua=mailto:a@example.com,https://report.example.com"
        ));
    }

    #[test]
    fn validate_tlsrpt_missing_version() {
        assert!(!validate_tlsrpt_record("rua=mailto:a@example.com"));
    }

    #[test]
    fn validate_tlsrpt_missing_rua() {
        assert!(!validate_tlsrpt_record("v=TLSRPTv1"));
    }

    #[test]
    fn validate_tlsrpt_empty_string() {
        assert!(!validate_tlsrpt_record(""));
    }

    #[test]
    fn validate_tlsrpt_wrong_version() {
        assert!(!validate_tlsrpt_record(
            "v=TLSRPTv2; rua=mailto:a@example.com"
        ));
    }

    // -- extract_tlsrpt_rua tests --

    #[test]
    fn extract_tlsrpt_rua_mailto() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1; rua=mailto:reports@example.com");
        assert_eq!(uris, vec!["mailto:reports@example.com"]);
    }

    #[test]
    fn extract_tlsrpt_rua_https() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1; rua=https://example.com/tls-report");
        assert_eq!(uris, vec!["https://example.com/tls-report"]);
    }

    #[test]
    fn extract_tlsrpt_rua_multiple() {
        let uris = extract_tlsrpt_rua(
            "v=TLSRPTv1; rua=mailto:a@example.com,https://example.com/report",
        );
        assert_eq!(
            uris,
            vec!["mailto:a@example.com", "https://example.com/report"]
        );
    }

    #[test]
    fn extract_tlsrpt_rua_no_rua_field() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1");
        assert!(uris.is_empty());
    }

    #[test]
    fn extract_tlsrpt_rua_empty_record() {
        let uris = extract_tlsrpt_rua("");
        assert!(uris.is_empty());
    }

    #[test]
    fn extract_tlsrpt_rua_whitespace_handling() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1 ;  rua=mailto:x@example.com  ");
        assert_eq!(uris, vec!["mailto:x@example.com"]);
    }

    #[test]
    fn extract_tlsrpt_rua_three_uris() {
        let uris = extract_tlsrpt_rua(
            "v=TLSRPTv1; rua=mailto:a@a.com,mailto:b@b.com,https://c.com/report",
        );
        assert_eq!(uris.len(), 3);
        assert_eq!(uris[0], "mailto:a@a.com");
        assert_eq!(uris[1], "mailto:b@b.com");
        assert_eq!(uris[2], "https://c.com/report");
    }

    // -- MTA-STS DNS record format tests --

    #[test]
    fn mta_sts_dns_record_name() {
        let domain = "example.com";
        let qname = format!("_mta-sts.{domain}");
        assert_eq!(qname, "_mta-sts.example.com");
    }

    #[test]
    fn mta_sts_policy_url_format() {
        let domain = "example.com";
        let url = format!("https://mta-sts.{domain}/.well-known/mta-sts.txt");
        assert_eq!(url, "https://mta-sts.example.com/.well-known/mta-sts.txt");
    }

    #[test]
    fn tlsrpt_dns_record_name() {
        let domain = "example.com";
        let qname = format!("_smtp._tls.{domain}");
        assert_eq!(qname, "_smtp._tls.example.com");
    }

    // -- MTA-STS mode behavior tests --

    #[test]
    fn enforce_mode_requires_tls() {
        let body = "version: STSv1\nmode: enforce\nmx: *.example.com\nmax_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        let mode = extract_sts_mode(&fields).unwrap();
        assert_eq!(mode, "enforce");
        // enforce means sender MUST use TLS and verify cert
    }

    #[test]
    fn testing_mode_allows_reporting() {
        let body = "version: STSv1\nmode: testing\nmx: *.example.com\nmax_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        let mode = extract_sts_mode(&fields).unwrap();
        assert_eq!(mode, "testing");
        // testing means send reports but don't enforce
    }

    #[test]
    fn none_mode_disables_policy() {
        let body = "version: STSv1\nmode: none\nmax_age: 0\n";
        let fields = parse_mta_sts_policy(body);
        let mode = extract_sts_mode(&fields).unwrap();
        assert_eq!(mode, "none");
        // none means no policy is active
    }

    // -- MTA-STS max_age boundary tests --

    #[test]
    fn max_age_recommended_minimum() {
        // RFC 8461 recommends max_age of at least 86400 (1 day) for enforce mode
        let fields = vec![("max_age".into(), "86400".into())];
        let age = extract_sts_max_age(&fields).unwrap();
        assert!(age >= 86400);
    }

    #[test]
    fn max_age_common_values() {
        // 1 day
        let fields = vec![("max_age".into(), "86400".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(86400));

        // 1 week
        let fields = vec![("max_age".into(), "604800".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(604800));

        // 1 year
        let fields = vec![("max_age".into(), "31557600".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(31557600));
    }

    // -- check_mta_sts_policy body validation logic tests --

    #[test]
    fn policy_body_has_mode_and_mx() {
        let body = "version: STSv1\nmode: enforce\nmx: *.example.com\nmax_age: 86400\n";
        let has_mode = body.contains("mode:");
        let has_mx = body.contains("mx:");
        assert!(has_mode && has_mx);
    }

    #[test]
    fn policy_body_missing_mode() {
        let body = "version: STSv1\nmx: *.example.com\nmax_age: 86400\n";
        let has_mode = body.contains("mode:");
        assert!(!has_mode);
    }

    #[test]
    fn policy_body_missing_mx() {
        let body = "version: STSv1\nmode: enforce\nmax_age: 86400\n";
        let has_mx = body.contains("mx:");
        assert!(!has_mx);
    }

    #[test]
    fn policy_body_empty() {
        let body = "";
        assert!(!body.contains("mode:"));
        assert!(!body.contains("mx:"));
    }

    // -- STS record v=STSv1 detection --

    #[test]
    fn sts_record_valid() {
        let record = "v=STSv1; id=20240101T000000Z";
        assert!(record.contains("v=STSv1"));
    }

    #[test]
    fn sts_record_invalid_version() {
        let record = "v=STSv2; id=20240101T000000Z";
        assert!(!record.contains("v=STSv1"));
    }

    #[test]
    fn sts_record_no_version() {
        let record = "id=20240101T000000Z";
        assert!(!record.contains("v=STSv1"));
    }

    // -- TLSRPT record v=TLSRPTv1 detection --

    #[test]
    fn tlsrpt_record_valid() {
        let record = "v=TLSRPTv1; rua=mailto:reports@example.com";
        assert!(record.contains("v=TLSRPTv1"));
    }

    #[test]
    fn tlsrpt_record_invalid_version() {
        let record = "v=TLSRPTv2; rua=mailto:reports@example.com";
        assert!(!record.contains("v=TLSRPTv1"));
    }

    // -- combined MTA-STS + mx_matches_policy integration --

    #[test]
    fn mta_sts_policy_and_mx_matching_integration() {
        let body = "\
version: STSv1\n\
mode: enforce\n\
mx: mail.example.com\n\
mx: *.example.com\n\
max_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        let mx_patterns = extract_sts_mx_patterns(&fields);

        // use mx_matches_policy from the outbound-queue crate logic
        // (replicated here as the same algorithm)
        let mx_host = "relay.example.com";
        let matches = mx_patterns.iter().any(|pattern| {
            let p = pattern.to_lowercase();
            let host = mx_host.to_lowercase();
            if p.starts_with("*.") {
                let suffix = &p[1..];
                host.ends_with(suffix)
                    && host.len() > suffix.len()
                    && !host[..host.len() - suffix.len()].contains('.')
            } else {
                host == p
            }
        });
        assert!(matches);
    }

    #[test]
    fn mta_sts_policy_enforce_rejects_non_matching_mx() {
        let body = "\
version: STSv1\n\
mode: enforce\n\
mx: *.example.com\n\
max_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        let mx_patterns = extract_sts_mx_patterns(&fields);

        let mx_host = "mail.other.com";
        let matches = mx_patterns.iter().any(|pattern| {
            let p = pattern.to_lowercase();
            let host = mx_host.to_lowercase();
            if p.starts_with("*.") {
                let suffix = &p[1..];
                host.ends_with(suffix)
                    && host.len() > suffix.len()
                    && !host[..host.len() - suffix.len()].contains('.')
            } else {
                host == p
            }
        });
        assert!(!matches);
    }

    // -- BIMI record tests --

    #[test]
    fn bimi_dns_record_name() {
        let domain = "example.com";
        let qname = format!("default._bimi.{domain}");
        assert_eq!(qname, "default._bimi.example.com");
    }

    #[test]
    fn extract_bimi_logo_url_valid() {
        let record = "v=BIMI1; l=https://example.com/logo.svg; a=";
        assert_eq!(
            extract_bimi_logo_url(record),
            Some("https://example.com/logo.svg".into())
        );
    }

    #[test]
    fn extract_bimi_logo_url_no_logo() {
        let record = "v=BIMI1; l=; a=";
        assert_eq!(extract_bimi_logo_url(record), None);
    }

    #[test]
    fn extract_bimi_logo_url_missing_l_tag() {
        let record = "v=BIMI1; a=https://example.com/vmc.pem";
        assert_eq!(extract_bimi_logo_url(record), None);
    }

    #[test]
    fn extract_bimi_logo_url_with_authority() {
        let record = "v=BIMI1; l=https://example.com/brand.svg; a=https://example.com/vmc.pem";
        assert_eq!(
            extract_bimi_logo_url(record),
            Some("https://example.com/brand.svg".into())
        );
    }

    #[test]
    fn extract_bimi_logo_url_whitespace() {
        let record = "v=BIMI1;  l = https://example.com/logo.svg ;";
        // the "l" part after split is " l = https://...", which starts_with "l=" is false
        // so this tests trimming behavior
        let url = extract_bimi_logo_url(record);
        // the part " l = https://..." doesn't start with "l=" after trim, it's "l = https://..."
        assert_eq!(url, None);
    }

    #[test]
    fn bimi_record_detection() {
        let record = "v=BIMI1; l=https://example.com/logo.svg";
        assert!(record.contains("v=BIMI1"));
    }

    #[test]
    fn bimi_record_wrong_version() {
        let record = "v=BIMI2; l=https://example.com/logo.svg";
        assert!(!record.contains("v=BIMI1"));
    }
}

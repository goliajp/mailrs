use std::net::IpAddr;

use hickory_resolver::proto::rr::RecordType;
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

    DomainCheckReport {
        domain: domain.to_string(),
        checks,
        checked_at: chrono::Utc::now().timestamp(),
    }
}

async fn check_mx(resolver: &TokioResolver, domain: &str, hostname: &str) -> CheckResult {
    match resolver.mx_lookup(domain).await {
        Ok(records) => {
            let entries: Vec<String> = records
                .iter()
                .map(|mx| format!("{} (priority {})", mx.exchange(), mx.preference()))
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
            let points_to_us = records.iter().any(|mx| {
                let exchange = mx.exchange().to_string();
                let exchange = exchange.trim_end_matches('.');
                exchange.eq_ignore_ascii_case(hostname)
            });
            if points_to_us {
                CheckResult {
                    name: "MX Records".into(),
                    status: Status::Pass,
                    message: format!("{} MX record(s) found, includes {hostname}",entries.len()),
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
                .iter()
                .map(|r| r.to_string())
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
                        (Status::Pass, format!("SPF record found, {policy_note}, includes {hostname}"))
                    } else {
                        (Status::Warn, format!("SPF record found, {policy_note}, but does not include {hostname}"))
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

async fn check_dkim(
    resolver: &TokioResolver,
    domain: &str,
    selector: Option<&str>,
) -> CheckResult {
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
                .iter()
                .map(|r| r.to_string())
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
                .iter()
                .map(|r| r.to_string())
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
                .iter()
                .map(|r| r.to_string())
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
                .iter()
                .map(|r| r.to_string())
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
            let ptrs: Vec<String> = names.iter().map(|n| n.to_string()).collect();
            let matches = ptrs
                .iter()
                .any(|n| n.trim_end_matches('.') == hostname);
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

async fn check_dane(resolver: &TokioResolver, domain: &str) -> CheckResult {
    // look up MX first, then check TLSA for port 25 on first MX
    let mx_host = match resolver.mx_lookup(domain).await {
        Ok(records) => {
            let mut entries: Vec<_> = records.iter().collect();
            entries.sort_by_key(|mx| mx.preference());
            entries
                .first()
                .map(|mx| mx.exchange().to_string().trim_end_matches('.').to_string())
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
                .iter()
                .map(|r| format!("{}", r))
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

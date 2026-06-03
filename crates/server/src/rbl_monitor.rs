use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use hickory_resolver::TokioResolver;
use serde::Serialize;

const CHECK_INTERVAL: Duration = Duration::from_secs(3600); // 1 hour

/// well-known RBL providers
const RBL_ZONES: &[(&str, &str)] = &[
    ("zen.spamhaus.org", "Spamhaus ZEN"),
    ("b.barracudacentral.org", "Barracuda"),
    ("bl.spamcop.net", "SpamCop"),
    ("dnsbl.sorbs.net", "SORBS"),
    ("dnsbl-1.uceprotect.net", "UCEPROTECT L1"),
];

#[derive(Debug, Clone, Serialize)]
pub struct RblResult {
    pub zone: String,
    pub name: String,
    pub listed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RblCheckReport {
    pub ip: String,
    pub checked_at: i64,
    pub results: Vec<RblResult>,
    pub any_listed: bool,
}

/// reverse an IPv4 address for DNSBL query (e.g. 1.2.3.4 -> 4.3.2.1)
fn reverse_ip(ip: &IpAddr) -> Option<String> {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            Some(format!(
                "{}.{}.{}.{}",
                octets[3], octets[2], octets[1], octets[0]
            ))
        }
        IpAddr::V6(_) => None, // most RBLs only support IPv4
    }
}

/// check a single IP against all configured RBL zones
async fn check_ip(resolver: &TokioResolver, ip: &IpAddr) -> Vec<RblResult> {
    let reversed = match reverse_ip(ip) {
        Some(r) => r,
        None => return Vec::new(),
    };

    let mut results = Vec::with_capacity(RBL_ZONES.len());

    for &(zone, name) in RBL_ZONES {
        let query = format!("{reversed}.{zone}");
        let listed = resolver
            .lookup_ip(&query)
            .await
            .map(|lookup| lookup.iter().next().is_some())
            .unwrap_or(false);

        results.push(RblResult {
            zone: zone.to_string(),
            name: name.to_string(),
            listed,
        });
    }

    results
}

/// start background RBL monitoring task
pub fn start(
    resolver: Arc<TokioResolver>,
    hostname: String,
    kevy: Option<redis::aio::ConnectionManager>,
) {
    tokio::spawn(async move {
        // initial delay to let services start
        tokio::time::sleep(Duration::from_secs(30)).await;

        let mut interval = tokio::time::interval(CHECK_INTERVAL);
        loop {
            interval.tick().await;

            // resolve our hostname to get outbound IP(s)
            let ips: Vec<IpAddr> = match resolver.lookup_ip(&hostname).await {
                Ok(lookup) => lookup.iter().collect(),
                Err(e) => {
                    tracing::warn!(
                        event = "rbl_resolve_failed",
                        hostname = %hostname,
                        error = %e
                    );
                    continue;
                }
            };

            for ip in &ips {
                let results = check_ip(&resolver, ip).await;
                let any_listed = results.iter().any(|r| r.listed);

                let report = RblCheckReport {
                    ip: ip.to_string(),
                    checked_at: chrono::Utc::now().timestamp(),
                    results,
                    any_listed,
                };

                if any_listed {
                    let listed: Vec<_> = report.results.iter().filter(|r| r.listed).collect();
                    let zones = listed
                        .iter()
                        .map(|r| r.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    tracing::warn!(
                        event = "rbl_alert",
                        ip = %ip,
                        zones = %zones,
                        "outbound IP is RBL-listed"
                    );
                }

                // store result in kevy
                if let Some(ref kevy) = kevy
                    && let Ok(json) = serde_json::to_string(&report)
                {
                    let key = format!("rbl:status:{ip}");
                    let _ = redis::cmd("SET")
                        .arg(&key)
                        .arg(&json)
                        .arg("EX")
                        .arg(7200i64) // 2 hour TTL
                        .query_async::<()>(&mut kevy.clone())
                        .await;
                }
            }
        }
    });
}

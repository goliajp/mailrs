//! What receiving servers say about our sending addresses.
//!
//! The primary signal is our own: a refusal whose reply names a blocklist
//! or our reputation is recorded with the server and its words, and the
//! operators are told once an hour per server. A second, optional source
//! is a daily lookup of our addresses (`MAILRS_EGRESS_IPS`) in the DNS
//! blocklists the deployment names (`MAILRS_EGRESS_DNSBL_ZONES`); with no
//! zones the check is off and says so once. Neither pauses anyone: only
//! our own bounce and complaint numbers do.

use std::sync::OnceLock;

use mailrs_auth_guard::alerts::{Alert, Fact, Kind};
use mailrs_core_sidestate::families::egress;
use mailrs_smtp_client::TokioResolver;
use tokio::task::spawn_blocking;

use super::{Cfg, Outcome, kevy, now_secs};

const DAY: i64 = 86_400;

/// The sender's once-a-minute housekeeping: the pause sweep, and the
/// daily blocklist check when it is due.
pub(super) async fn every_minute(cfg: &Cfg) {
    super::pause::sweep(cfg).await;
    zone_check_if_due(cfg).await;
}

/// The refused delivery's reply, recorded and announced when it names a
/// blocklist or our reputation.
pub(super) fn note_refusal(cfg: &Cfg, outcome: &Outcome) {
    let reply = match outcome {
        Outcome::Permanent(r) | Outcome::Transient(r) => r,
        Outcome::Delivered => return,
    };
    let Some(mention) = egress::listing_mention(reply) else {
        return;
    };
    // replies read "<mx> <code> <text>"; ours carry no host
    let host = reply.split_whitespace().next().unwrap_or("").to_string();
    let r = egress::Refusal {
        at: now_secs(),
        host: host.clone(),
        mention: mention.to_string(),
        reply: reply.chars().take(500).collect(),
    };
    tracing::warn!(event = "egress_refused", %host, mention, reply = %r.reply, "a receiver refused our mail, naming a blocklist or our reputation");
    let url = cfg.kevy_url.clone();
    tokio::spawn(async move {
        let alert = Alert {
            kind: Kind::EgressRefused,
            subject: host.clone(),
            at: r.at.max(0) as u64,
            detail: r.reply.clone(),
            facts: vec![Fact::RemoteRefusal {
                host,
                reply: r.reply.chars().take(300).collect(),
            }],
        };
        let res = spawn_blocking(move || {
            let mut c = kevy(&url)?;
            egress::record_refusal(&mut c, &r)?;
            mailrs_auth_guard::alerts::raise(&mut c, &alert)
        })
        .await;
        if !matches!(res, Ok(Ok(_))) {
            tracing::warn!(?res, "egress refusal not recorded");
        }
    });
}

struct ZoneConfig {
    ips: Vec<String>,
    zones: Vec<String>,
}

fn zone_config() -> &'static ZoneConfig {
    static CONFIG: OnceLock<ZoneConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let list = |name: &str| -> Vec<String> {
            std::env::var(name)
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        };
        let c = ZoneConfig {
            ips: list("MAILRS_EGRESS_IPS"),
            zones: list("MAILRS_EGRESS_DNSBL_ZONES"),
        };
        if c.zones.is_empty() || c.ips.is_empty() {
            tracing::warn!(
                ips = c.ips.len(),
                zones = c.zones.len(),
                "egress blocklist check is off: MAILRS_EGRESS_IPS or MAILRS_EGRESS_DNSBL_ZONES is empty; refusals are still read"
            );
        }
        c
    })
}

/// The query name for `ip` in `zone`, or `None` for an address that is
/// not IPv4 (the lists checked here are IPv4 lists).
fn query_name(ip: &str, zone: &str) -> Option<String> {
    let v4: std::net::Ipv4Addr = ip.parse().ok()?;
    let [a, b, c, d] = v4.octets();
    Some(format!("{d}.{c}.{b}.{a}.{zone}."))
}

/// Whether an answer says "listed": `127.0.0.2`–`127.0.0.255`. Lists
/// answer `127.255.255.x` to say the query itself was refused (a public
/// resolver, over quota), which is an error, not a listing.
fn listing_answer(a: std::net::Ipv4Addr) -> Option<Result<(), String>> {
    let [x, y, z, w] = a.octets();
    match (x, y, z) {
        (127, 255, 255) => Some(Err(format!("query refused by the list ({a})"))),
        (127, 0, 0) if w >= 2 => Some(Ok(())),
        _ => None,
    }
}

/// Once a day, when configured: look our addresses up in each zone,
/// store the result for the admin page, and alert on each listing.
async fn zone_check_if_due(cfg: &Cfg) {
    let conf = zone_config();
    if conf.zones.is_empty() || conf.ips.is_empty() {
        return;
    }
    let url = cfg.kevy_url.clone();
    let last = spawn_blocking(move || -> std::io::Result<i64> {
        let raw = kevy(&url)?.get(egress::RBL_STATUS)?;
        Ok(raw
            .and_then(|b| serde_json::from_slice::<egress::ZoneCheck>(&b).ok())
            .map_or(0, |z| z.checked_at))
    })
    .await;
    let now = now_secs();
    match last {
        Ok(Ok(at)) if now - at < DAY => return,
        Ok(Ok(_)) => {}
        other => {
            tracing::warn!(?other, "egress blocklist check: last run unknown; skipped");
            return;
        }
    }
    let resolver = match TokioResolver::builder_tokio().and_then(|b| b.build()) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "egress blocklist check: no resolver");
            return;
        }
    };
    let mut check = egress::ZoneCheck {
        checked_at: now,
        ips: conf.ips.clone(),
        zones: conf.zones.clone(),
        listed: Vec::new(),
        errors: Vec::new(),
    };
    for ip in &conf.ips {
        for zone in &conf.zones {
            let Some(name) = query_name(ip, zone) else {
                check.errors.push(format!("{ip}: not an IPv4 address"));
                continue;
            };
            match resolver.lookup_ip(name.as_str()).await {
                Ok(answers) => {
                    for a in answers.iter().filter_map(|a| match a {
                        std::net::IpAddr::V4(v4) => Some(v4),
                        std::net::IpAddr::V6(_) => None,
                    }) {
                        match listing_answer(a) {
                            Some(Ok(())) => check.listed.push(egress::Listed {
                                ip: ip.clone(),
                                zone: zone.clone(),
                                answer: a.to_string(),
                            }),
                            Some(Err(e)) => check.errors.push(format!("{zone}: {e}")),
                            None => {}
                        }
                    }
                }
                Err(e) if e.is_no_records_found() => {}
                Err(e) => check.errors.push(format!("{zone}: {e}")),
            }
        }
    }
    tracing::info!(
        event = "egress_zone_check",
        listed = check.listed.len(),
        errors = check.errors.len(),
        "egress blocklist check done"
    );
    let url = cfg.kevy_url.clone();
    let res = spawn_blocking(move || -> std::io::Result<()> {
        let mut c = kevy(&url)?;
        egress::record_zone_check(&mut c, &check)?;
        for l in &check.listed {
            let alert = Alert {
                kind: Kind::EgressListed,
                subject: l.ip.clone(),
                at: check.checked_at.max(0) as u64,
                detail: format!("{} is listed in {} ({})", l.ip, l.zone, l.answer),
                facts: vec![Fact::Listed {
                    address: l.ip.clone(),
                    zone: l.zone.clone(),
                }],
            };
            mailrs_auth_guard::alerts::raise(&mut c, &alert)?;
        }
        Ok(())
    })
    .await;
    if !matches!(res, Ok(Ok(()))) {
        tracing::warn!(?res, "egress blocklist check not stored");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_names_reverse_the_address() {
        assert_eq!(
            query_name("52.195.89.111", "bl.example").as_deref(),
            Some("111.89.195.52.bl.example.")
        );
        assert_eq!(query_name("::1", "bl.example"), None);
    }

    #[test]
    fn only_loopback_answers_from_two_up_are_listings() {
        let a = |s: &str| listing_answer(s.parse().unwrap());
        assert_eq!(a("127.0.0.2"), Some(Ok(())));
        assert_eq!(a("127.0.0.11"), Some(Ok(())));
        assert!(matches!(a("127.255.255.254"), Some(Err(_))));
        assert_eq!(a("127.0.0.1"), None);
        assert_eq!(a("10.0.0.2"), None);
    }
}

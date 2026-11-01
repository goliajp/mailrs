//! What receiving servers say about our sending addresses.
//!
//! ```text
//! egress:refusals:<YYYYMMDD>  list  Refusal (JSON), one per refusal that
//!                             names a blocklist or our reputation; kept
//!                             180 days
//! admin:rbl:status            string ZoneCheck (JSON): the latest daily
//!                             check of our addresses against the DNS
//!                             blocklists the deployment names (a report,
//!                             overwritten)
//! ```
//!
//! A refusal is our own evidence: the receiver that would not take our
//! mail said why. The zone check is a second, optional source; neither
//! pauses anyone on its own.

use std::time::Duration;

use kevy_client::Connection;

const REFUSALS: &str = "egress:refusals:";
/// The latest zone check, read by the admin page.
pub const RBL_STATUS: &[u8] = b"admin:rbl:status";
const REFUSALS_TTL: Duration = Duration::from_secs(180 * 86_400);

/// Words in a refusal that name a blocklist or our address's reputation,
/// lowercase. Matched as substrings of the lowercased reply.
const MENTIONS: &[&str] = &[
    "blocklist",
    "block list",
    "blacklist",
    "black list",
    "blocked using",
    "listed at",
    "listed by",
    "listed in",
    "listed on",
    "spamhaus",
    "barracuda",
    "spamcop",
    "sorbs",
    "dnsbl",
    "rbl",
    "ip reputation",
    "poor reputation",
    "sender reputation",
    "reputation of",
    "s3140",
];

/// The first mention in `reply` of a blocklist or our reputation.
pub fn listing_mention(reply: &str) -> Option<&'static str> {
    let lower = reply.to_ascii_lowercase();
    MENTIONS.iter().copied().find(|m| {
        // "rbl" alone is too short to trust inside words ("verbless")
        match *m {
            "rbl" => lower
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|w| w == "rbl"),
            _ => lower.contains(m),
        }
    })
}

/// One refusal that named a blocklist or our reputation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Refusal {
    /// Unix seconds.
    pub at: i64,
    /// The server that refused.
    pub host: String,
    /// What it named.
    pub mention: String,
    /// Its reply, at most 500 bytes.
    pub reply: String,
}

fn day(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|d| d.format("%Y%m%d").to_string())
        .unwrap_or_default()
}

/// Record `r`.
///
/// # Errors
/// The store could not be written.
pub fn record_refusal(c: &mut Connection, r: &Refusal) -> std::io::Result<()> {
    let key = format!("{REFUSALS}{}", day(r.at));
    let bytes = serde_json::to_vec(r).map_err(std::io::Error::other)?;
    c.rpush(key.as_bytes(), &[bytes.as_slice()])?;
    c.expire(key.as_bytes(), REFUSALS_TTL)?;
    Ok(())
}

/// The refusals recorded on the UTC day holding `at`, oldest first.
///
/// # Errors
/// The store could not be read.
pub fn refusals_on(c: &mut Connection, at: i64) -> std::io::Result<Vec<Refusal>> {
    let key = format!("{REFUSALS}{}", day(at));
    Ok(c.lrange(key.as_bytes(), 0, -1)?
        .iter()
        .filter_map(|b| serde_json::from_slice(b).ok())
        .collect())
}

/// One address found on one list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Listed {
    pub ip: String,
    pub zone: String,
    /// The list's answer, `127.0.0.x`.
    pub answer: String,
}

/// The latest daily zone check.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ZoneCheck {
    pub checked_at: i64,
    pub ips: Vec<String>,
    pub zones: Vec<String>,
    pub listed: Vec<Listed>,
    /// Lookups that failed, `zone: error`; a failed lookup is not a
    /// clean answer.
    pub errors: Vec<String>,
}

/// Overwrite the latest zone check.
///
/// # Errors
/// The store could not be written.
pub fn record_zone_check(c: &mut Connection, z: &ZoneCheck) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(z).map_err(std::io::Error::other)?;
    c.set(RBL_STATUS, &bytes)?;
    Ok(())
}

/// The latest zone check, or `None` before the first.
///
/// # Errors
/// The store could not be read.
pub fn read_zone_check(c: &mut Connection) -> std::io::Result<Option<ZoneCheck>> {
    Ok(c.get(RBL_STATUS)?
        .and_then(|b| serde_json::from_slice(&b).ok()))
}

/// A check older than this has not run when it should have (daily).
pub const STALE_AFTER_SECS: i64 = 36 * 3600;

/// `/api/health`'s `egress`: whether an outgoing address is on a list, which
/// lists, when last checked, and whether that check is overdue. `listed`,
/// `checked_at` and `stale` are null before the first check.
pub fn health(check: Option<&ZoneCheck>, now: i64) -> serde_json::Value {
    let Some(z) = check else {
        return serde_json::json!({
            "listed": null,
            "zones_listed": [],
            "checked_at": null,
            "stale": null,
        });
    };
    let mut zones: Vec<&str> = z.listed.iter().map(|l| l.zone.as_str()).collect();
    zones.sort_unstable();
    zones.dedup();
    serde_json::json!({
        "listed": !z.listed.is_empty(),
        "zones_listed": zones,
        "checked_at": z.checked_at,
        "stale": now - z.checked_at > STALE_AFTER_SECS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(at: i64, zones: &[&str]) -> ZoneCheck {
        ZoneCheck {
            checked_at: at,
            ips: vec!["192.0.2.1".into()],
            zones: vec!["a.example".into(), "b.example".into()],
            listed: zones
                .iter()
                .map(|z| Listed {
                    ip: "192.0.2.1".into(),
                    zone: z.to_string(),
                    answer: "127.0.0.2".into(),
                })
                .collect(),
            errors: Vec::new(),
        }
    }

    #[test]
    fn health_says_listed_which_lists_and_whether_the_check_is_overdue() {
        let none = health(None, 100);
        assert!(
            none["listed"].is_null() && none["stale"].is_null() && none["checked_at"].is_null()
        );
        let clean = health(Some(&check(1_000, &[])), 1_000 + 3600);
        assert_eq!(clean["listed"], false);
        assert_eq!(clean["zones_listed"], serde_json::json!([]));
        assert_eq!(clean["stale"], false);
        let listed = health(
            Some(&check(1_000, &["b.example", "a.example", "b.example"])),
            1_000,
        );
        assert_eq!(listed["listed"], true);
        assert_eq!(
            listed["zones_listed"],
            serde_json::json!(["a.example", "b.example"])
        );
        let old = health(Some(&check(1_000, &[])), 1_000 + STALE_AFTER_SECS + 1);
        assert_eq!(old["stale"], true);
    }

    #[test]
    fn mentions_are_found_and_ordinary_refusals_are_not() {
        for (reply, want) in [
            (
                "550 5.7.1 Service unavailable; Client host [52.195.89.111] blocked using zen.spamhaus.org",
                "blocked using",
            ),
            (
                "550 5.7.1 Unfortunately, messages from [52.195.89.111] weren't sent. ... (S3140)",
                "s3140",
            ),
            ("554 Your IP is listed on our RBL", "listed on"),
            ("550 RBL: rejected", "rbl"),
            (
                "421 4.7.0 IP not in whitelist, poor reputation",
                "poor reputation",
            ),
        ] {
            assert_eq!(listing_mention(reply), Some(want), "{reply}");
        }
        for reply in [
            "550 5.1.1 User unknown",
            "552 mailbox full",
            "550 verbless nonsense",
        ] {
            assert_eq!(listing_mention(reply), None, "{reply}");
        }
    }

    #[test]
    fn refusals_are_kept_by_day() {
        let mut c = Connection::connect("mem://egress-refusals").unwrap();
        let r = Refusal {
            at: 1_800_000_000,
            host: "mx.example".into(),
            mention: "rbl".into(),
            reply: "550 RBL".into(),
        };
        record_refusal(&mut c, &r).unwrap();
        assert_eq!(refusals_on(&mut c, 1_800_000_000).unwrap(), vec![r]);
        assert!(
            refusals_on(&mut c, 1_800_000_000 + 86_400)
                .unwrap()
                .is_empty()
        );
    }
}

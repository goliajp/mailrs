//! The screening rules, in shadow: the reputation and attachment facts of
//! every message that reaches the content checks go to the Lua rules, and
//! each hit is logged, never acted on, until the replay report has been
//! reviewed.

use std::sync::{Arc, OnceLock};

use mailrs_core_sidestate::families::screening::{self, Live};
use mailrs_inbound::ReceiveContext;

use crate::kevy_net::KevyNetClient;

static LIVE: OnceLock<Live> = OnceLock::new();
static OWN: OnceLock<Vec<String>> = OnceLock::new();
static KEVY: OnceLock<Arc<KevyNetClient>> = OnceLock::new();
/// When each address was last recorded as abusive, so one source sending
/// many refused commands becomes one observation a day.
static ABUSE_SEEN: std::sync::Mutex<Option<std::collections::HashMap<std::net::IpAddr, i64>>> =
    std::sync::Mutex::new(None);
const ABUSE_EVERY_SECS: i64 = 86_400;

/// How often the sets are checked for new observations.
const REFRESH_SECS: u64 = 60;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Load the sets, and keep them current from the shared store.
pub fn spawn_refresh(kevy: Arc<KevyNetClient>, own_domains: Vec<String>) {
    let _ = OWN.set(own_domains.iter().map(|d| d.to_ascii_lowercase()).collect());
    let _ = LIVE.set(Live::new(own_domains));
    let _ = KEVY.set(kevy.clone());
    tokio::spawn(async move {
        loop {
            let k = kevy.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let live = LIVE.get().expect("set above");
                let version = mailrs_screening_lua::active_version();
                k.with_conn(|c| {
                    live.refresh(c, now());
                    let report = screening::RulesReport {
                        approved: mailrs_screening_lua::approved(&version),
                        version: version.clone(),
                        failures_total: live.failures_total(),
                        reported_at: now(),
                    };
                    if let Err(e) = screening::record_rules_report(c, &report) {
                        tracing::warn!(error = %e, "the screening rules report was not written");
                    }
                    Ok(())
                })
            })
            .await;
            tokio::time::sleep(std::time::Duration::from_secs(REFRESH_SECS)).await;
        }
    });
}

fn domain_of(address: &str) -> String {
    address
        .trim()
        .trim_matches(['<', '>'])
        .rsplit_once('@')
        .map(|(_, d)| d.to_ascii_lowercase())
        .unwrap_or_default()
}

/// What the screening rules would decide for `rc`.
pub(crate) fn evaluate(
    live: &Live,
    rc: &ReceiveContext,
) -> Result<mailrs_screening_lua::Verdict, String> {
    let parsed = mailrs_mime::parse(&rc.message);
    let m = mailrs_reputation::Message {
        client_ip: Some(rc.client_ip),
        envelope_domain: domain_of(&rc.sender),
        header_domain: domain_of(&rc.header_from_addr),
        link_hosts: mailrs_inbound::scan_input::link_hosts(&parsed),
        auth: mailrs_reputation::Auth {
            dmarc_fail: rc.auth_results.dmarc == "fail",
            spf_fail: rc.auth_results.spf == "fail",
        },
    };
    let reputation = live.reputation().facts(&m);
    let parts = mailrs_inbound::scan_input::attachments(&parsed);
    let atts: Vec<mailrs_attachment_scan::Attachment> = parts
        .iter()
        .map(|p| mailrs_attachment_scan::Attachment {
            name: &p.name,
            declared_type: &p.declared_type,
            bytes: p.bytes,
        })
        .collect();
    let parts = live.inspector().inspect(&atts);
    mailrs_screening_lua::evaluate(&mailrs_screening_lua::Input {
        outside: outside(rc),
        parts: &parts,
        reputation: &reputation,
    })
}

/// Run the rules on `rc` and log what they would have done. A failure or
/// a panic is counted and logged; the mail is never held up.
pub fn shadow(rc: &ReceiveContext) {
    let Some(live) = LIVE.get() else {
        return;
    };
    if let Some(reason) = poses_as_ours(rc) {
        record_abuse(rc.client_ip, reason);
    }
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| evaluate(live, rc)));
    let verdict = match run {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            live.failed("rules", &e);
            return;
        }
        Err(_) => {
            live.failed("facts", "an extractor panicked");
            return;
        }
    };
    if verdict.hits.is_empty() {
        return;
    }
    let message_id = message_id(&rc.message);
    for h in &verdict.hits {
        tracing::info!(
            event = "screening_shadow",
            %message_id,
            rule = %h.rule,
            kind = %h.kind,
            action = ?h.action,
            score = h.score,
            detail = %h.detail,
            evidence = %h.evidence.join(" | "),
            version = %verdict.version,
            "screening (shadow)"
        );
    }
}

/// Record `ip` as having abused this server, at most once a day per
/// address; private and loopback addresses are not recorded. The write
/// runs off the session's task.
pub fn record_abuse(ip: std::net::IpAddr, reason: String) {
    if !public(ip) || !first_today(ip, now()) {
        return;
    }
    let Some(kevy) = KEVY.get().cloned() else {
        return;
    };
    tokio::task::spawn_blocking(move || {
        let o = mailrs_reputation::Observation {
            id: String::new(),
            at: now(),
            source: mailrs_reputation::Source::Abuse,
            kind: mailrs_reputation::Kind::Ip,
            value: ip.to_string(),
            stance: mailrs_reputation::Stance::Deny,
            retracts: None,
            reason,
            actor: None,
        };
        if let Err(e) = kevy.with_conn(|c| screening::add_observation(c, o)) {
            tracing::warn!(event = "screening_failed", stage = "observe", error = %e, %ip, "an abuse observation was not recorded");
        }
    });
}

fn public(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            !(v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified())
        }
        std::net::IpAddr::V6(v6) => {
            !(v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00)
        }
    }
}

fn first_today(ip: std::net::IpAddr, now: i64) -> bool {
    let mut seen = ABUSE_SEEN.lock().expect("lock");
    let map = seen.get_or_insert_with(Default::default);
    if map.get(&ip).is_some_and(|at| now - at < ABUSE_EVERY_SECS) {
        return false;
    }
    map.retain(|_, at| now - *at < ABUSE_EVERY_SECS);
    map.insert(ip, now);
    true
}

/// Mail that claims one of our domains in `From:` and fails DMARC.
fn poses_as_ours(rc: &ReceiveContext) -> Option<String> {
    let domain = domain_of(&rc.header_from_addr);
    let ours = OWN.get().is_some_and(|own| own.contains(&domain));
    (ours && rc.auth_results.dmarc.eq_ignore_ascii_case("fail"))
        .then(|| format!("posed as {domain} and failed DMARC"))
}

/// The sender is not one of ours: the From domain is another's, or ours
/// without passing DMARC (someone using our name).
fn outside(rc: &ReceiveContext) -> bool {
    let domain = domain_of(&rc.header_from_addr);
    let ours = OWN.get().is_some_and(|own| own.contains(&domain));
    !(ours && rc.auth_results.dmarc == "pass")
}

/// The Message-ID header's value, for the log; empty when there is none.
pub(crate) fn message_id(raw: &[u8]) -> String {
    let head = &raw[..raw.len().min(16 * 1024)];
    String::from_utf8_lossy(head)
        .lines()
        .take_while(|l| !l.is_empty())
        .find(|l| l.len() > 11 && l[..11].eq_ignore_ascii_case("message-id:"))
        .map(|l| l[11..].trim().trim_matches(['<', '>']).to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(message: &[u8], from: &str, dmarc: &str) -> ReceiveContext {
        let mut rc = ReceiveContext::new(
            "200.181.82.134".parse().unwrap(),
            "mx.example",
            from,
            "qa@golia.jp",
            message.to_vec(),
            "mail.golia.ai",
        );
        rc.auth_results.dmarc = dmarc.into();
        rc
    }

    #[test]
    fn an_incident_address_and_a_program_are_both_seen() {
        let _ = OWN.set(vec!["golia.jp".into()]);
        let live = Live::new(vec!["golia.jp".into()]);
        let mut c = kevy_client::Connection::connect("mem://receiver-screening").unwrap();
        mailrs_core_sidestate::families::screening::add_observation(
            &mut c,
            mailrs_reputation::Observation {
                id: String::new(),
                at: now(),
                source: mailrs_reputation::Source::Incident,
                kind: mailrs_reputation::Kind::Ip,
                value: "200.181.82.134".into(),
                stance: mailrs_reputation::Stance::Deny,
                retracts: None,
                reason: "10-24 incident".into(),
                actor: None,
            },
        )
        .unwrap();
        live.refresh(&mut c, now());
        let mail = b"From: x@outside.example\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nsee https://a.example/x\r\n--b\r\nContent-Type: application/octet-stream; name=invoice.pdf.exe\r\nContent-Disposition: attachment; filename=invoice.pdf.exe\r\n\r\nMZ\r\n--b--\r\n";
        let v = evaluate(&live, &context(mail, "x@outside.example", "none")).unwrap();
        let rules: Vec<&str> = v.hits.iter().map(|h| h.rule.as_str()).collect();
        assert_eq!(
            rules,
            ["exe-name", "double-extension", "ip-incident-manual"]
        );
        assert!(v.hits[2].evidence[0].starts_with("r1 (incident, 0 days)"));
        assert!(v.version.starts_with("lua:"));
        // a colleague's own mail, passing DMARC, is not from outside
        let mut ours = context(mail, "y@golia.jp", "pass");
        ours.header_from_addr = "y@golia.jp".into();
        let v = evaluate(&live, &ours).unwrap();
        let rules: Vec<&str> = v.hits.iter().map(|h| h.rule.as_str()).collect();
        assert_eq!(rules, ["double-extension", "ip-incident-manual"]);
    }

    #[test]
    fn the_message_id_is_read_from_the_head_only() {
        assert_eq!(
            message_id(b"Subject: x\r\nMessage-ID: <a@b>\r\n\r\nMessage-ID: <c@d>"),
            "a@b"
        );
        assert_eq!(message_id(b"Subject: x\r\n\r\nMessage-ID: <c@d>"), "");
        assert_eq!(domain_of("<Ann@Example.ORG>"), "example.org");
    }

    #[test]
    fn abuse_is_recorded_once_a_day_and_only_for_public_addresses() {
        let ip: std::net::IpAddr = "198.51.100.7".parse().unwrap();
        assert!(first_today(ip, 1_000));
        assert!(!first_today(ip, 1_000 + ABUSE_EVERY_SECS - 1));
        assert!(first_today(ip, 1_000 + ABUSE_EVERY_SECS));
        assert!(public(ip));
        for private in ["10.0.0.1", "127.0.0.1", "192.168.1.1", "::1", "fd00::1"] {
            assert!(!public(private.parse().unwrap()), "{private}");
        }
    }

    #[test]
    fn posing_as_our_domain_needs_a_dmarc_fail() {
        let _ = OWN.set(vec!["golia.jp".into()]);
        let mut rc = context(b"From: ceo@golia.jp\r\n\r\nx", "x@else.example", "fail");
        rc.header_from_addr = "ceo@golia.jp".into();
        assert_eq!(
            poses_as_ours(&rc).as_deref(),
            Some("posed as golia.jp and failed DMARC")
        );
        rc.auth_results.dmarc = "pass".into();
        assert_eq!(poses_as_ours(&rc), None);
        rc.auth_results.dmarc = "temperror".into();
        assert_eq!(poses_as_ours(&rc), None);
    }
}

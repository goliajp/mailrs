//! Outgoing mail through the attachment rules.
//!
//! Each job is read once, on its first attempt. The attachment facts go to
//! the same Lua rules the receiver runs, with the sender counted as one of
//! ours — so only the rules that hold for colleagues' mail too can fire
//! (a name that hides its type, a program named as a document, a PDF or
//! picture that runs script, HTML that builds a file, a hash recorded as
//! malicious). Every hit is logged. A job is refused only while the rule
//! bundle in use is one a replay review approved; until then this is a
//! shadow, like the receiver's.
//!
//! The fraud rules read the same message, and are only logged: a rule
//! written for mail from strangers has not been measured on our own
//! people's mail, and refusing on it waits for that review.
//!
//! Forwarded mail was screened when it arrived, and security alerts must
//! leave; neither is read.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mailrs_attachment_scan::Inspector;
use mailrs_core_sidestate::families::outbound::SECURITY_ALERT_KIND;
use mailrs_screening_lua::Action;

use super::{Cfg, kevy, now_secs};

/// How long one build of the hash list is used.
const INSPECTOR_TTL: Duration = Duration::from_secs(60);

static INSPECTOR: Mutex<Option<(Instant, Arc<Inspector>)>> = Mutex::new(None);

/// The reason to refuse `message`, when an approved bundle's attachment
/// rule asks for it; `None` sends it on.
pub(super) async fn refusal(
    cfg: &Cfg,
    id: &str,
    envelope: &serde_json::Value,
    message: &[u8],
    first_attempt: bool,
) -> Option<String> {
    let forwarded = envelope
        .get("original_sender")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty());
    let alert = envelope.get("kind").and_then(|v| v.as_str()) == Some(SECURITY_ALERT_KIND);
    if !first_attempt || forwarded || alert {
        return None;
    }
    let inspector = inspector(cfg).await?;
    let (id, message) = (id.to_string(), message.to_vec());
    let judged = tokio::task::spawn_blocking(move || {
        log_fraud(&id, &message);
        judge(&inspector, &message).map(|v| (id, v))
    })
    .await;
    let (id, verdict) = match judged {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            tracing::warn!(event = "screening_failed", stage = "outbound", error = %e, "outgoing mail was not screened; sending");
            return None;
        }
        Err(e) => {
            tracing::warn!(event = "screening_failed", stage = "outbound", error = %e, "outgoing screening panicked; sending");
            return None;
        }
    };
    let hits = refusing_hits(&verdict);
    if hits.is_empty() {
        return None;
    }
    let approved = mailrs_screening_lua::approved(&verdict.version);
    for h in &hits {
        tracing::warn!(event = "outbound_screening", %id, rule = %h.rule, detail = %h.detail, evidence = %h.evidence.join(" | "), version = %verdict.version, enforced = approved, "an outgoing attachment matched a rule");
    }
    approved.then(|| format!("not sent: {} ({})", hits[0].reason, hits[0].detail))
}

/// The fraud rules' findings on an outgoing message, logged only.
fn log_fraud(id: &str, message: &[u8]) {
    match mailrs_fastcore::outbound_fraud::judge(message) {
        Ok(scan) if scan.findings.is_empty() => {}
        Ok(scan) => {
            let rules: Vec<&str> = scan.findings.iter().map(|f| f.rule.as_str()).collect();
            tracing::info!(event = "outbound_fraud_shadow", %id, rules = %rules.join(","), score = scan.findings.score(), holds = scan.findings.hold_worthy(), version = %scan.version, "outgoing mail matched fraud rules (shadow)");
        }
        Err(e) => {
            tracing::warn!(event = "screening_failed", stage = "outbound_fraud", error = %e, "outgoing mail was not read by the fraud rules")
        }
    }
}

/// Attachment rules that ask for Junk: on outgoing mail, a refusal.
fn refusing_hits(v: &mailrs_screening_lua::Verdict) -> Vec<&mailrs_screening_lua::Hit> {
    v.hits
        .iter()
        .filter(|h| h.kind == "attachment" && h.action == Action::Junk)
        .collect()
}

fn judge(inspector: &Inspector, message: &[u8]) -> Result<mailrs_screening_lua::Verdict, String> {
    let parsed = mailrs_mime::parse(message);
    let parts = mailrs_inbound::scan_input::attachments(&parsed);
    if parts.is_empty() {
        return Ok(mailrs_screening_lua::Verdict {
            hits: Vec::new(),
            version: String::new(),
        });
    }
    let atts: Vec<mailrs_attachment_scan::Attachment> = parts
        .iter()
        .map(|p| mailrs_attachment_scan::Attachment {
            name: &p.name,
            declared_type: &p.declared_type,
            bytes: p.bytes,
        })
        .collect();
    let facts = inspector.inspect(&atts);
    let reputation = mailrs_reputation::Facts::default();
    mailrs_screening_lua::evaluate(&mailrs_screening_lua::Input {
        outside: false,
        parts: &facts,
        reputation: &reputation,
    })
}

/// The hash list, rebuilt from the shared store at most once a minute;
/// the last build stays in use when the store cannot be read.
async fn inspector(cfg: &Cfg) -> Option<Arc<Inspector>> {
    let cached = INSPECTOR.lock().expect("lock").clone();
    if let Some((at, i)) = &cached
        && at.elapsed() < INSPECTOR_TTL
    {
        return Some(i.clone());
    }
    let url = cfg.kevy_url.clone();
    let built = tokio::task::spawn_blocking(move || {
        let mut c = kevy(&url)?;
        let obs = mailrs_core_sidestate::families::screening::hash_observations(&mut c)?;
        Ok::<_, std::io::Error>(Inspector::build(&obs, Default::default(), now_secs()))
    })
    .await;
    match built {
        Ok(Ok(i)) => {
            let i = Arc::new(i);
            *INSPECTOR.lock().expect("lock") = Some((Instant::now(), i.clone()));
            Some(i)
        }
        other => {
            tracing::warn!(
                event = "screening_failed",
                stage = "build",
                ?other,
                "outgoing hash list not rebuilt"
            );
            cached.map(|(_, i)| i)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(name: &str, body: &[u8]) -> Vec<u8> {
        let mut m = format!(
            "From: a@golia.jp\r\nTo: b@example.org\r\nSubject: x\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n\
             --b\r\nContent-Type: text/plain\r\n\r\nhi\r\n--b\r\nContent-Type: application/octet-stream; name=\"{name}\"\r\n\
             Content-Disposition: attachment; filename=\"{name}\"\r\n\r\n"
        )
        .into_bytes();
        m.extend_from_slice(body);
        m.extend_from_slice(b"\r\n--b--\r\n");
        m
    }

    fn rules(message: &[u8]) -> Vec<String> {
        let v = judge(&Inspector::build(&[], Default::default(), 0), message).unwrap();
        refusing_hits(&v).iter().map(|h| h.rule.clone()).collect()
    }

    #[test]
    fn a_colleagues_program_goes_and_a_disguised_one_is_caught() {
        assert!(
            rules(&mail("setup.exe", b"MZ")).is_empty(),
            "a named program from one of ours"
        );
        assert_eq!(rules(&mail("invoice.pdf.exe", b"MZ")), ["double-extension"]);
        assert_eq!(rules(&mail("report.pdf", b"MZ\x90\x00")), ["type-mismatch"]);
        assert!(rules(b"From: a@golia.jp\r\n\r\nplain letter\r\n").is_empty());
    }

    #[test]
    fn a_hash_recorded_as_malicious_is_caught_whatever_its_name() {
        use sha2::{Digest, Sha256};
        let bytes = b"just some bytes";
        let sha: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let obs = mailrs_attachment_scan::HashObservation {
            id: "h1".into(),
            at: 0,
            sha256: sha,
            stance: mailrs_attachment_scan::Stance::Deny,
            retracts: None,
            reason: "seen in an incident".into(),
            actor: None,
        };
        let v = judge(
            &Inspector::build(&[obs], Default::default(), 0),
            &mail("notes.txt", bytes),
        )
        .unwrap();
        let ids: Vec<&str> = refusing_hits(&v).iter().map(|h| h.rule.as_str()).collect();
        assert_eq!(ids, ["own-hash-listed"]);
    }

    #[test]
    fn the_fraud_rules_read_an_outgoing_letter() {
        let scan = mailrs_fastcore::outbound_fraud::judge(
            b"From: A <a@golia.jp>\r\nTo: b@example.org\r\nSubject: hi\r\n\r\nhello\r\n",
        )
        .unwrap();
        assert!(scan.version.starts_with("lua:"));
        assert!(!scan.findings.hold_worthy());
    }
}

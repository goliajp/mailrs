//! Recipients the sender never opens a connection for.
//!
//! `example.com` is reserved for documentation (RFC 2606) and publishes
//! a null MX (RFC 7505), so nothing can be delivered there. Test and
//! demo flows on this server address it on purpose, and each attempt
//! used to fail on DNS and stay in the queue for days. They are told
//! the mail was delivered and the job ends.
//!
//! Mail to the mock domain is stored in the recipient's mock namespace
//! here, instead of going out to the MX and back in through the receiver.

use mailrs_core_sidestate::families::mock::{self, Direction, MessageMeta, Stored};
use mailrs_core_sidestate::families::send as sendfam;

use super::config::Cfg;
use super::deliver::extract_addr_spec;
use super::outcome::{Outcome, report_send_outcome};
use super::queue::drop_blob;

pub(super) fn is_mock_recipient(recipient: &str) -> bool {
    let addr = extract_addr_spec(recipient);
    let Some((_, domain)) = addr.rsplit_once('@') else {
        return false;
    };
    let domain = domain.to_ascii_lowercase();
    domain == "example.com" || domain.ends_with(".example.com") || mock::is_mock(addr)
}

/// End the job without opening a connection.
pub(super) async fn report_delivered(
    cfg: Cfg,
    id: String,
    envelope: &serde_json::Value,
    sender: &str,
    recipient: &str,
    message: &[u8],
) {
    let send_id = envelope.get("send_id").and_then(|v| v.as_str());
    if let Some(sim) = mock::simulation(extract_addr_spec(recipient)) {
        let state = simulate(&cfg, sim, sender, recipient, message);
        record_state(&cfg, send_id, sender, state);
    } else if mock::is_mock(extract_addr_spec(recipient)) {
        let state = capture(&cfg, &id, recipient, message);
        record_state(&cfg, send_id, sender, state);
    } else {
        tracing::info!(%id, %recipient, "example.com: reported delivered, not sent");
        report_send_outcome(&cfg, send_id, sender, recipient, &Outcome::Delivered, 1);
    }
    if let Err(e) = drop_blob(cfg, id.clone()).await {
        tracing::error!(%id, err = %e, "drop_blob after mocked delivery failed");
    }
}

// the queue id names the message, so a job picked up twice stores one copy
fn capture(cfg: &Cfg, id: &str, recipient: &str, message: &[u8]) -> sendfam::RecipientState {
    let rcpt = extract_addr_spec(recipient).to_ascii_lowercase();
    let state = |delivered: bool, code: u16, message: String| sendfam::RecipientState {
        recipient: recipient.to_string(),
        delivered,
        pending: false,
        code,
        message,
    };
    let Some(addr) = mock::parse(&rcpt) else {
        return state(false, 550, "550 5.1.1 not a valid mock address".into());
    };
    let ns = addr.namespace.clone();
    let meta = MessageMeta {
        id: mock::spool_message_id(&format!("queue:{id}"), 0),
        namespace: addr.namespace,
        tag: addr.tag,
        direction: Direction::In,
        rcpt: rcpt.clone(),
        size: 0,
        received_at: 0,
        uid: 0,
        summary: mailrs_mock::parse_message(message).summary,
    };
    let stored = super::kevy(&cfg.kevy_url)
        .map_err(|e| e.to_string())
        .and_then(|mut c| {
            mock::store_message(&mut c, mock::root(), meta, message).map_err(|e| e.to_string())
        });
    match stored {
        Ok(Stored::Stored | Stored::AlreadyStored) => {
            tracing::info!(%id, %rcpt, "mock: captured, not sent");
            state(true, 250, format!("captured by mock namespace {ns}"))
        }
        Ok(Stored::NoNamespace) => state(false, 550, format!("550 5.1.1 no mock namespace {ns}")),
        Ok(Stored::Full) => state(false, 452, "452 4.3.1 mock mail storage is full".into()),
        Ok(Stored::Disabled) => state(false, 550, "550 5.1.1 mock mail is turned off".into()),
        Err(e) => {
            tracing::warn!(%id, %rcpt, error = %e, "mock: capture failed");
            state(false, 451, format!("451 4.3.0 mock store unavailable: {e}"))
        }
    }
}

// the answer goes into the sender's own INBOX; the suppression list and
// the feedback loop never hear about it
fn simulate(
    cfg: &Cfg,
    sim: mailrs_mock::Simulation,
    sender: &str,
    recipient: &str,
    message: &[u8],
) -> sendfam::RecipientState {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let rcpt = extract_addr_spec(recipient);
    let reply = mailrs_mock::simulation::reply(sim, mock::domain(), sender, rcpt, message, now);
    let queued = super::kevy(&cfg.kevy_url)
        .and_then(|mut c| mailrs_fastcore::bounce_queue::enqueue_notice(&mut c, sender, &reply));
    if let Err(e) = queued {
        tracing::warn!(%recipient, error = %e, "mock: simulated reply not queued");
    }
    let status = sim.status_line();
    sendfam::RecipientState {
        recipient: recipient.to_string(),
        delivered: sim.delivered(),
        pending: false,
        code: status[..3].parse().unwrap_or(0),
        message: status.to_string(),
    }
}

fn record_state(cfg: &Cfg, send_id: Option<&str>, user: &str, state: sendfam::RecipientState) {
    let Some(send_id) = send_id else {
        return;
    };
    let written = super::kevy(&cfg.kevy_url).and_then(|mut c| {
        sendfam::update_recipient(&mut c, user, send_id, &state)?;
        Ok(())
    });
    if let Err(e) = written {
        tracing::warn!(err = %e, %send_id, "send row outcome write failed");
    }
}

#[cfg(test)]
mod tests {
    use super::is_mock_recipient;

    #[test]
    fn example_com_and_its_subdomains_are_mocked() {
        assert!(is_mock_recipient("alex@example.com"));
        assert!(is_mock_recipient("<Alex@Example.COM>"));
        assert!(is_mock_recipient("a@mail.example.com"));
    }

    #[test]
    fn other_domains_are_sent_for_real() {
        assert!(!is_mock_recipient("29841300@qq.com"));
        assert!(!is_mock_recipient("a@notexample.com"));
        assert!(!is_mock_recipient("a@example.com.cn"));
        assert!(!is_mock_recipient("a@example.org"));
    }
}

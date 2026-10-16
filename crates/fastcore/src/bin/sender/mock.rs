//! Recipients reported delivered without being sent to.
//!
//! `example.com` is reserved for documentation (RFC 2606) and publishes
//! a null MX (RFC 7505), so nothing can be delivered there. Test and
//! demo flows on this server address it on purpose, and each attempt
//! used to fail on DNS and stay in the queue for days. They are told
//! the mail was delivered and the job ends.

use super::config::Cfg;
use super::deliver::extract_addr_spec;
use super::outcome::{Outcome, report_send_outcome};
use super::queue::drop_blob;

pub(super) fn is_mock_recipient(recipient: &str) -> bool {
    let Some((_, domain)) = extract_addr_spec(recipient).rsplit_once('@') else {
        return false;
    };
    let domain = domain.to_ascii_lowercase();
    domain == "example.com" || domain.ends_with(".example.com")
}

/// End the job as delivered without opening a connection.
pub(super) async fn report_delivered(
    cfg: Cfg,
    id: String,
    envelope: &serde_json::Value,
    sender: &str,
    recipient: &str,
) {
    tracing::info!(%id, %recipient, "example.com: reported delivered, not sent");
    let send_id = envelope.get("send_id").and_then(|v| v.as_str());
    report_send_outcome(&cfg, send_id, sender, recipient, &Outcome::Delivered, 1);
    if let Err(e) = drop_blob(cfg, id.clone()).await {
        tracing::error!(%id, err = %e, "drop_blob after mocked delivery failed");
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

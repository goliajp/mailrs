//! Per-MX SMTP delivery with STARTTLS / DANE policy handling.

use std::sync::Arc;

use hickory_resolver::TokioResolver;

use crate::queue::QueuedMessage;
use crate::{DeliveryEvent, DeliveryEventSender, TlsAttemptOutcome};

/// TLS policy for outbound connections
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsPolicy {
    /// try STARTTLS, fall back to plaintext on failure (default)
    Opportunistic,
    /// require TLS, fail delivery if STARTTLS unavailable or fails
    Require,
}

/// try to deliver messages via a specific MX host
///
/// `port` is the TCP port to connect to on `mx_host`. Production
/// always passes 25 (the SMTP relay port); integration tests inject
/// an ephemeral mock-server port. Kept as a parameter rather than
/// hardcoded so the worker stays testable end-to-end without a real
/// MTA on port 25.
#[allow(clippy::too_many_arguments)]
pub async fn try_deliver_via_mx(
    hostname: &str,
    mx_host: &str,
    port: u16,
    domain: &str,
    messages: &[QueuedMessage],
    resolver: &TokioResolver,
    event_sender: Option<&DeliveryEventSender>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    try_deliver_via_mx_with_tls(
        hostname,
        mx_host,
        port,
        domain,
        messages,
        TlsPolicy::Opportunistic,
        None,
        resolver,
        event_sender,
    )
    .await
}

/// Try to deliver messages via a specific MX host with explicit TLS
/// policy and optional ClientConfig override.
///
/// `tls_config_override` lets integration tests inject a dangerous
/// (skip-verify) `rustls::ClientConfig` so the STARTTLS-success path
/// can be driven against a mock SMTP server presenting a self-signed
/// cert. Production code (`try_deliver_via_mx`) passes `None` so the
/// default `webpki-roots` PKIX verifier is used.
#[allow(clippy::too_many_arguments)]
pub async fn try_deliver_via_mx_with_tls(
    hostname: &str,
    mx_host: &str,
    port: u16,
    domain: &str,
    messages: &[QueuedMessage],
    tls_policy: TlsPolicy,
    tls_config_override: Option<Arc<rustls::ClientConfig>>,
    resolver: &TokioResolver,
    event_sender: Option<&DeliveryEventSender>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use mailrs_smtp_client::StarttlsResult;

    // Helper to emit a TlsAttempt event with the given outcome.
    let emit_tls = |outcome: TlsAttemptOutcome| {
        if let Some(es) = event_sender {
            es(DeliveryEvent::TlsAttempt {
                domain: domain.to_string(),
                mx_host: mx_host.to_string(),
                outcome,
            });
        }
    };

    let mut smtp = mailrs_smtp_client::SmtpConnection::connect(mx_host, port).await?;
    let ehlo_resp = smtp.ehlo(hostname).await?;

    if !ehlo_resp.is_positive() {
        return Err(format!("EHLO rejected: {}", ehlo_resp.message()).into());
    }

    // resolve TLSA records for DANE
    let tlsa_records = mailrs_smtp_client::resolve_tlsa(resolver, mx_host).await;
    let has_dane = !tlsa_records.is_empty();
    if has_dane {
        tracing::debug!("found {} TLSA records for {mx_host}", tlsa_records.len());
    }

    // try STARTTLS if advertised
    if ehlo_resp.has_extension("STARTTLS") {
        let tls_result = if has_dane {
            // use DANE-verified TLS
            smtp.try_starttls_dane(mx_host, tlsa_records).await
        } else if let Some(ref cfg) = tls_config_override {
            // caller-supplied ClientConfig (typically a test
            // harness with a skip-verify verifier; never used in
            // production paths)
            smtp.try_starttls_with_config(mx_host, (**cfg).clone())
                .await
        } else {
            // standard PKIX TLS
            smtp.try_starttls(mx_host).await
        };

        match tls_result {
            StarttlsResult::Success(tls_smtp) => {
                smtp = tls_smtp;
                let _ = smtp.ehlo(hostname).await?;
                let policy: &'static str = if has_dane {
                    tracing::debug!("DANE-verified TLS established with {mx_host}");
                    "dane"
                } else {
                    tracing::debug!("TLS established with {mx_host}");
                    "opportunistic"
                };
                emit_tls(TlsAttemptOutcome::Success { policy });
            }
            StarttlsResult::Rejected {
                conn,
                code,
                message,
            } => {
                emit_tls(TlsAttemptOutcome::Rejected {
                    code,
                    message: message.clone(),
                });
                if has_dane || tls_policy == TlsPolicy::Require {
                    return Err(format!(
                        "STARTTLS rejected by {mx_host} ({code}): {message}{}",
                        if has_dane {
                            " (DANE required)"
                        } else {
                            " (TLS required)"
                        }
                    )
                    .into());
                }
                tracing::warn!(
                    "STARTTLS rejected by {mx_host} ({code}): {message}; continuing in plain"
                );
                // Connection is still usable in plain mode per
                // StarttlsResult::Rejected contract.
                smtp = conn;
            }
            StarttlsResult::HandshakeFailed { outcome, source } => {
                emit_tls(TlsAttemptOutcome::HandshakeFailed(outcome.clone()));
                if has_dane || tls_policy == TlsPolicy::Require {
                    return Err(format!(
                        "STARTTLS handshake failed for {mx_host} ({}): {source}{}",
                        outcome.as_str(),
                        if has_dane {
                            " (DANE required)"
                        } else {
                            " (TLS required)"
                        }
                    )
                    .into());
                }
                tracing::warn!(
                    "STARTTLS handshake failed for {mx_host} ({}): {source}; reconnecting in plain",
                    outcome.as_str()
                );
                smtp = mailrs_smtp_client::SmtpConnection::connect(mx_host, port).await?;
                let resp = smtp.ehlo(hostname).await?;
                if !resp.is_positive() {
                    return Err(format!("EHLO rejected on reconnect: {}", resp.message()).into());
                }
            }
        }
    } else if has_dane || tls_policy == TlsPolicy::Require {
        emit_tls(TlsAttemptOutcome::NotAdvertised);
        return Err(format!(
            "{mx_host} does not advertise STARTTLS{}",
            if has_dane {
                " (DANE TLSA records present, TLS required)"
            } else {
                " and TLS is required"
            }
        )
        .into());
    } else {
        emit_tls(TlsAttemptOutcome::NotAdvertised);
        tracing::info!("delivering to {mx_host} without TLS (STARTTLS not advertised)");
    }

    for msg in messages {
        let to = [msg.recipient.as_str()];
        let resp = smtp.deliver(&msg.sender, &to, &msg.message_data).await?;
        if !resp.is_positive() {
            return Err(format!("delivery failed: {}", resp.message()).into());
        }
    }

    let _ = smtp.quit().await;
    Ok(())
}

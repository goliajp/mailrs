//! One MX host: connect, settle TLS under the domain's policy, hand the
//! message over.

use mailrs_smtp_client::{MxRecord, StarttlsResult, TlsOutcome};

use super::super::config::*;
use super::super::*;

/// Everything fixed for one delivery while its MX hosts are tried in turn.
pub(super) struct Attempt<'a> {
    pub(super) cfg: &'a Cfg,
    pub(super) resolver: &'a TokioResolver,
    pub(super) domain: &'a str,
    pub(super) sender: &'a str,
    pub(super) recipient: &'a str,
    pub(super) payload: &'a [u8],
    pub(super) sts_policy: Option<&'a mailrs_mta_sts::Policy>,
    pub(super) sts_enforce: bool,
    pub(super) timeouts: &'a TimeoutConfig,
}

impl Attempt<'_> {
    fn record_tls(
        &self,
        mx: &MxRecord,
        success: bool,
        failure_type: Option<String>,
        detail: Option<String>,
    ) {
        mailrs_fastcore::tlsrpt::record(
            &self.cfg.kevy_url,
            &mailrs_fastcore::tlsrpt::TlsEvent {
                domain: self.domain.to_string(),
                mx: mx.exchange.to_string(),
                success,
                failure_type,
                detail,
            },
        );
    }

    /// Try one MX. `None` means go on to the next, with `last_err` saying why.
    pub(super) async fn mx(&self, mx: &MxRecord, last_err: &mut String) -> Option<Outcome> {
        let cfg = self.cfg;
        // enforce: skip MX not covered by the policy's mx: patterns
        if let Some(policy) = self.sts_policy
            && self.sts_enforce
            && mailrs_fastcore::sender_sts::mx_decision(policy, &mx.exchange)
                == mailrs_mta_sts::Decision::Deny
        {
            *last_err = format!("mta-sts enforce: {} not in policy mx:", mx.exchange);
            tracing::warn!(err = %last_err, "MX excluded by STS policy, next MX");
            self.record_tls(
                mx,
                false,
                Some("mx-mismatch".into()),
                Some(last_err.clone()),
            );
            return None;
        }
        tracing::info!(mx = %mx.exchange, priority = mx.priority, recipient = %self.recipient, "attempt");
        let conn = match SmtpConnection::connect_with_timeout(&mx.exchange, 25, self.timeouts).await
        {
            Ok(c) => c,
            Err(e) => {
                *last_err = format!("connect {}: {e}", mx.exchange);
                tracing::warn!(err = %last_err, "connect failed, next MX");
                return None;
            }
        };

        // First EHLO (plain).
        let mut conn = conn;
        if let Err(e) = conn.ehlo(&cfg.helo).await {
            *last_err = format!("ehlo {}: {e}", mx.exchange);
            tracing::warn!(err = %last_err, "ehlo failed, next MX");
            return None;
        }

        let (conn, tls_used) = self.settle_tls(conn, mx, last_err).await?;
        self.hand_over(conn, mx, tls_used, last_err).await
    }

    /// The connection to deliver on, and whether it ended up encrypted.
    async fn settle_tls(
        &self,
        conn: SmtpConnection,
        mx: &MxRecord,
        last_err: &mut String,
    ) -> Option<(SmtpConnection, bool)> {
        let cfg = self.cfg;
        // Opportunistic STARTTLS with plaintext downgrade on failure.
        //
        // - Success: upgrade + re-EHLO
        // - Rejected (server refused STARTTLS): stay on the same plaintext conn
        // - HandshakeFailed (peer cert expired / SNI mismatch / etc.):
        //   the TCP session is dead, so open a fresh plaintext session
        //   and continue there. This matches how Gmail/O365/Postfix
        //   handle opportunistic-TLS failures — SPF/DKIM/DMARC (not TLS)
        //   are the real integrity guarantees for interpersonal mail.
        // DANE (RFC 7672 / G8.2): if the MX publishes DNSSEC-anchored
        // TLSA records, TLS is mandatory and the cert is verified
        // against them — a missing/failed handshake must NOT downgrade.
        let tlsa = mailrs_smtp_client::resolve_tlsa(self.resolver, &mx.exchange).await;
        let dane_active = !tlsa.is_empty();
        let starttls = if dane_active {
            let cfg = mailrs_smtp_client::dane_tls_config(tlsa);
            conn.try_starttls_with_config(&mx.exchange, cfg).await
        } else {
            conn.try_starttls(&mx.exchange).await
        };
        match starttls {
            StarttlsResult::Success(c) => {
                let mut c = c;
                if let Err(e) = c.ehlo(&cfg.helo).await {
                    *last_err = format!("ehlo-after-starttls {}: {e}", mx.exchange);
                    tracing::warn!(err = %last_err, "post-tls ehlo failed, next MX");
                    return None;
                }
                Some((c, true))
            }
            StarttlsResult::Rejected {
                conn,
                code,
                message: msg,
            } => {
                if self.sts_enforce || dane_active {
                    *last_err = format!("mta-sts enforce: {} refused STARTTLS", mx.exchange);
                    tracing::warn!(err = %last_err, "STARTTLS refused under STS enforce, next MX");
                    self.record_tls(
                        mx,
                        false,
                        Some("starttls-not-supported".into()),
                        Some(last_err.clone()),
                    );
                    let mut c = conn;
                    let _ = c.quit().await;
                    return None;
                }
                tracing::info!(code, %msg, "STARTTLS rejected, continuing plaintext");
                Some((conn, false))
            }
            StarttlsResult::HandshakeFailed { outcome, source } => {
                let plain = self
                    .after_failed_handshake(mx, dane_active, outcome, source, last_err)
                    .await?;
                Some((plain, false))
            }
        }
    }

    /// A handshake that broke the session: stop under a policy that
    /// forbids plaintext, otherwise reconnect without TLS.
    async fn after_failed_handshake(
        &self,
        mx: &MxRecord,
        dane_active: bool,
        outcome: TlsOutcome,
        source: std::io::Error,
        last_err: &mut String,
    ) -> Option<SmtpConnection> {
        let cfg = self.cfg;
        if self.sts_enforce || dane_active {
            // Say *why*. The stone classifies the failure and
            // this arm used to discard both the classification
            // and the error, so the log and the bounce both read
            // "TLS handshake failed" and nothing else — while
            // the branch one line down, the one that does not
            // stop delivery, logged the source.
            //
            // 2026-08-17: every message to every Microsoft 365
            // tenant was dying here, and the sentence that would
            // have said so in one line —
            // `CertificateNotTrusted("UnknownIssuer")` — was
            // computed and thrown away. It cost a bounce report,
            // a packet capture and an afternoon to recover a
            // string the process already had.
            *last_err = format!(
                "mta-sts enforce: {} TLS handshake failed: {}: {}",
                mx.exchange,
                outcome.as_str(),
                outcome.detail()
            );
            tracing::warn!(err = %last_err, "TLS handshake failed under STS enforce, next MX");
            // TLS-RPT exists for exactly this class, and the
            // sibling arm records one while this did not — so a
            // domain whose STARTTLS is refused was reported and
            // a domain whose certificate we cannot verify was
            // not, which is backwards. `source` is unused now
            // that `outcome` carries both the class and the
            // detail; it is the same string.
            //
            // The stone already classified this in RFC
            // 8460's own vocabulary — `as_str()` returns
            // `certificate-not-trusted`,
            // `certificate-host-mismatch` and the rest.
            // Writing a constant here would discard the
            // classification a second time, one layer up
            // from where it was discarded the first.
            self.record_tls(
                mx,
                false,
                Some(outcome.as_str().to_string()),
                Some(last_err.clone()),
            );
            return None;
        }
        tracing::warn!(
            err = %source,
            mx = %mx.exchange,
            "STARTTLS handshake failed, downgrading to plaintext"
        );
        let mut plain =
            match SmtpConnection::connect_with_timeout(&mx.exchange, 25, self.timeouts).await {
                Ok(c) => c,
                Err(e) => {
                    *last_err = format!("plaintext reconnect {}: {e}", mx.exchange);
                    tracing::warn!(err = %last_err, "reconnect after TLS failure failed, next MX");
                    return None;
                }
            };
        if let Err(e) = plain.ehlo(&cfg.helo).await {
            *last_err = format!("plaintext ehlo {}: {e}", mx.exchange);
            tracing::warn!(err = %last_err, "plaintext ehlo after TLS failure failed, next MX");
            return None;
        }
        Some(plain)
    }

    /// Send the message and read what the host said about it.
    async fn hand_over(
        &self,
        conn: SmtpConnection,
        mx: &MxRecord,
        tls_used: bool,
        last_err: &mut String,
    ) -> Option<Outcome> {
        let mut conn = conn;
        let resp = match conn
            .deliver(self.sender, &[self.recipient], self.payload)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                *last_err = format!("deliver {}: {e}", mx.exchange);
                tracing::warn!(err = %last_err, "deliver io error, next MX");
                let _ = conn.quit().await;
                return None;
            }
        };

        let _ = conn.quit().await;

        if resp.is_positive() {
            tracing::info!(mx = %mx.exchange, code = resp.code, msg = %resp.message(), "delivered");
            // TLS-RPT success event (G8.3): tls_used tracks whether the
            // final connection upgraded — set at STARTTLS resolution.
            self.record_tls(
                mx,
                tls_used,
                (!tls_used).then(|| "starttls-not-supported".to_string()),
                None,
            );
            return Some(Outcome::Delivered);
        }
        if resp.is_permanent_error() {
            let msg = format!("{} {} {}", mx.exchange, resp.code, resp.message());
            tracing::warn!(err = %msg, "permanent rejection");
            return Some(Outcome::Permanent(msg));
        }
        // Transient (4xx): try next MX before giving up on this attempt.
        *last_err = format!("{} {} {}", mx.exchange, resp.code, resp.message());
        tracing::warn!(err = %last_err, "transient rejection, next MX");
        None
    }
}

//! Minimal SMTP delivery for Phase 4.5.
//!
//! Resolves MX for the recipient domain, connects to the top MX on
//! port 25 (plain SMTP — staging is mostly internal relay; STARTTLS +
//! DKIM signing come with checklist 4.6–4.7), runs MAIL FROM / RCPT TO /
//! DATA / .QUIT, returns the SMTP response.
//!
//! Failure modes:
//! - DNS resolve error / no MX records  → `Err`, caller marks failed
//! - TCP connect error                  → `Err`, caller marks failed
//! - SMTP 4xx                           → `Ok(Reject::Transient)`, caller marks failed (retry)
//! - SMTP 5xx                           → `Ok(Reject::Permanent)`, caller marks bounced
//! - SMTP 2xx                           → `Ok(Reject::Accepted)`, caller marks delivered

use std::io;
use std::sync::Arc;

use hickory_resolver::TokioResolver;
use mailrs_smtp_client::{SmtpConnection, resolve_mx, sort_mx_records};

/// Final SMTP outcome.
#[derive(Debug)]
pub enum Outcome {
    /// 2xx — accepted by remote.
    Accepted,
    /// 4xx — temporary; sender should retry later.
    Transient(String),
    /// 5xx — permanent; sender should bounce.
    Permanent(String),
}

/// Deliver one envelope. Best-effort: tries the top MX only, plain SMTP
/// on :25. Returns an io error on transport failure (DNS / TCP).
pub async fn deliver_envelope(
    resolver: &Arc<TokioResolver>,
    from: &str,
    recipient: &str,
    message: &[u8],
    hostname: &str,
) -> io::Result<Outcome> {
    let domain = recipient
        .rsplit_once('@')
        .map(|(_, d)| d.to_string())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no @ in recipient"))?;

    // MX resolution
    let mut records = resolve_mx(resolver, &domain)
        .await
        .map_err(|e| io::Error::other(format!("MX resolve {domain} failed: {e}")))?;
    sort_mx_records(&mut records);
    let top = records.first().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("no MX records for {domain}"),
        )
    })?;

    let mut conn = SmtpConnection::connect(&top.exchange, 25).await?;
    let ehlo = conn.ehlo(hostname).await?;
    if !ehlo.is_positive() {
        return Ok(Outcome::Transient(format!("EHLO: {}", ehlo.message())));
    }

    let resp = conn.deliver(from, &[recipient], message).await?;

    let code = resp.code;
    let text = resp.message().to_string();
    if (200..300).contains(&code) {
        Ok(Outcome::Accepted)
    } else if (400..500).contains(&code) {
        Ok(Outcome::Transient(format!("{code} {text}")))
    } else {
        Ok(Outcome::Permanent(format!("{code} {text}")))
    }
}

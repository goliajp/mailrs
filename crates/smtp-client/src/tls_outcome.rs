//! Structured TLS outcomes for STARTTLS attempts.
//!
//! The 1.0 `starttls()` / `starttls_dane()` returned `io::Result<Self>`
//! — all signal collapsed to a single error string. 1.1 adds
//! [`try_starttls`](crate::SmtpConnection::try_starttls) and
//! [`try_starttls_dane`](crate::SmtpConnection::try_starttls_dane)
//! which return a [`StarttlsResult`] carrying either the upgraded
//! connection or a structured [`TlsOutcome`] suitable for direct
//! mapping into a TLSRPT report's `result-type` (RFC 8460 §4.3) and
//! for any downstream decision that wants to distinguish
//! certificate-expired from hostname-mismatched from DANE-rejected.
//!
//! ## Why a separate enum, not `mailrs_tls_rpt::FailureType`
//!
//! `mailrs-smtp-client` is a low-level SMTP-protocol stone. Pulling
//! `mailrs-tls-rpt` (which is itself a higher-level report
//! abstraction) into the smtp-client dep graph would invert the
//! stack — the report consumer should depend on the protocol
//! source, not the other way around. So `TlsOutcome` is defined
//! here and the caller (e.g. server-side
//! `outbound_tls_rpt::record_failure`) does the one-line mapping
//! to `FailureType`. That mapping is more flexible too: callers
//! that don't ship TLSRPT reports can still use `TlsOutcome` for
//! their own logging / metrics taxonomy.
//!
//! ## Classification rules
//!
//! The handshake failure category is derived by downcasting the
//! underlying `rustls::Error` when possible:
//!
//! - `InvalidCertificate(Expired | NotValidYet)` → [`TlsOutcome::CertificateExpired`]
//! - `InvalidCertificate(NotValidForName)` → [`TlsOutcome::CertificateHostMismatch`]
//! - `InvalidCertificate(UnknownIssuer | BadSignature | other path errors)` → [`TlsOutcome::CertificateNotTrusted`]
//! - `AlertReceived(BadCertificate)` → [`TlsOutcome::CertificateNotTrusted`]
//! - DANE-specific verifier rejections (`General(s)` with `"dane"` in `s`) → [`TlsOutcome::DaneValidationFailure`]
//!
//! Anything that can't be cleanly classified falls into
//! [`TlsOutcome::Other`] with the error string for diagnostics —
//! no information is lost.

use std::io;

use crate::SmtpConnection;

/// Structured outcome of a STARTTLS attempt.
///
/// Returned inside a [`StarttlsResult::HandshakeFailed`] variant
/// when TLS negotiation actually started but failed mid-handshake.
/// Variants align with the post-handshake `result-type` values from
/// RFC 8460 §4.3 plus a few smtp-client-specific failure modes
/// (`InvalidServerName`, `NetworkError`, `Other`) for cases the
/// RFC's vocabulary doesn't cleanly cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsOutcome {
    /// Server certificate is expired or not yet valid.
    /// Maps to RFC 8460 `certificate-expired`.
    CertificateExpired(String),

    /// Server certificate's subject / SAN doesn't include the MX
    /// hostname we connected to. Maps to RFC 8460
    /// `certificate-host-mismatch`.
    CertificateHostMismatch(String),

    /// PKIX path validation failed — unknown issuer, broken chain,
    /// untrusted root, bad signature, etc. Maps to RFC 8460
    /// `certificate-not-trusted`.
    CertificateNotTrusted(String),

    /// DANE TLSA verification rejected the server certificate.
    /// Only produced by [`crate::SmtpConnection::try_starttls_dane`].
    /// Maps to RFC 8460 `tlsa-invalid`.
    DaneValidationFailure(String),

    /// The hostname couldn't be turned into a valid TLS SNI value
    /// (typically because it was an IP literal or contained
    /// disallowed characters). Pre-handshake error; no RFC 8460
    /// vocabulary; reported as `validation-failure` by callers.
    InvalidServerName(String),

    /// Underlying TCP / IO error during the TLS handshake (peer
    /// closed, write timeout, etc.). No RFC 8460 vocabulary;
    /// callers typically report `validation-failure`.
    NetworkError(String),

    /// Catch-all for anything we couldn't cleanly classify.
    /// String preserves the underlying error verbatim for
    /// diagnostics. Callers typically report `validation-failure`.
    Other(String),
}

impl TlsOutcome {
    /// Short stable identifier suitable for tracing / metric labels.
    /// Matches the RFC 8460 §4.3 string where applicable.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CertificateExpired(_) => "certificate-expired",
            Self::CertificateHostMismatch(_) => "certificate-host-mismatch",
            Self::CertificateNotTrusted(_) => "certificate-not-trusted",
            Self::DaneValidationFailure(_) => "tlsa-invalid",
            Self::InvalidServerName(_) => "invalid-server-name",
            Self::NetworkError(_) => "network-error",
            Self::Other(_) => "other",
        }
    }

    /// Underlying diagnostic detail (the wrapped string).
    pub fn detail(&self) -> &str {
        match self {
            Self::CertificateExpired(s)
            | Self::CertificateHostMismatch(s)
            | Self::CertificateNotTrusted(s)
            | Self::DaneValidationFailure(s)
            | Self::InvalidServerName(s)
            | Self::NetworkError(s)
            | Self::Other(s) => s,
        }
    }
}

/// Result of one STARTTLS attempt.
///
/// Discriminates between four distinct outcomes:
///
/// 1. **Success**: connection upgraded to TLS, caller proceeds.
/// 2. **Rejected**: server refused the `STARTTLS` command (4xx/5xx
///    response). The plain connection is still usable — caller can
///    EHLO again and continue without TLS if policy allows.
/// 3. **HandshakeFailed**: TLS negotiation started but didn't
///    complete. The connection is unrecoverable; caller must
///    reconnect from scratch.
pub enum StarttlsResult {
    /// TLS handshake completed. The wrapped connection is now
    /// encrypted; caller should re-EHLO over the new channel.
    Success(SmtpConnection),

    /// `STARTTLS` command was rejected by the server. Connection
    /// is still in plain state; caller can keep using it.
    Rejected {
        /// Original plain connection — still usable for non-TLS
        /// delivery if the caller's TLS policy permits.
        conn: SmtpConnection,
        /// SMTP response code (e.g. `454`, `502`).
        code: u16,
        /// SMTP response text.
        message: String,
    },

    /// TLS negotiation began but failed before completing. The
    /// underlying TCP stream is no longer usable; the caller must
    /// reconnect.
    HandshakeFailed {
        /// Structured failure classification, suitable for direct
        /// mapping to `mailrs_tls_rpt::FailureType`.
        outcome: TlsOutcome,
        /// The original `io::Error` that triggered the failure,
        /// preserved for diagnostics / log messages.
        source: io::Error,
    },
}

impl StarttlsResult {
    /// `true` if the handshake succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    /// Convenience: extract the upgraded connection, discarding
    /// the failure context. Used by the 1.0 `starttls()`-shaped API
    /// that lives on top of `try_starttls` for backwards compat.
    pub fn into_io_result(self) -> io::Result<SmtpConnection> {
        match self {
            Self::Success(c) => Ok(c),
            Self::Rejected { code, message, .. } => Err(io::Error::other(format!(
                "STARTTLS rejected ({code}): {message}"
            ))),
            Self::HandshakeFailed { outcome, source } => Err(io::Error::other(format!(
                "STARTTLS handshake failed ({}): {source}",
                outcome.as_str()
            ))),
        }
    }
}

/// Classify a TLS-handshake `io::Error` into a [`TlsOutcome`].
///
/// Walks the error chain looking for a `rustls::Error`; if found,
/// uses its detailed sub-variants. Otherwise falls back to the
/// `kind()`-based heuristic.
///
/// `is_dane` is set by the DANE-specific entry point so DANE
/// rejections get the `DaneValidationFailure` variant rather than
/// being lumped into `CertificateNotTrusted`. The default for the
/// PKIX path is `false`.
pub(crate) fn classify_io_error(e: &io::Error, is_dane: bool) -> TlsOutcome {
    // 1. Try downcasting through the error chain to rustls::Error.
    let mut maybe = e.get_ref().and_then(|r| r.downcast_ref::<rustls::Error>());
    let mut depth = 0;
    while maybe.is_none() && depth < 4 {
        // Walk source chain manually since rustls::Error doesn't
        // always sit at depth 0 (tokio_rustls may wrap it).
        depth += 1;
        let source = e.get_ref().and_then(|r| std::error::Error::source(r));
        if let Some(s) = source {
            if let Some(r) = s.downcast_ref::<rustls::Error>() {
                maybe = Some(r);
                break;
            }
        } else {
            break;
        }
    }
    if let Some(r) = maybe {
        return classify_rustls_error(r, is_dane);
    }

    // 2. Fall back to io::ErrorKind for plain TCP-level failures.
    match e.kind() {
        io::ErrorKind::TimedOut
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof => TlsOutcome::NetworkError(e.to_string()),
        io::ErrorKind::InvalidInput => TlsOutcome::InvalidServerName(e.to_string()),
        _ => TlsOutcome::Other(e.to_string()),
    }
}

/// Classify a [`rustls::Error`] directly.
pub(crate) fn classify_rustls_error(e: &rustls::Error, is_dane: bool) -> TlsOutcome {
    use rustls::CertificateError;

    match e {
        rustls::Error::InvalidCertificate(cert_err) => match cert_err {
            CertificateError::Expired | CertificateError::NotValidYet => {
                TlsOutcome::CertificateExpired(format!("{cert_err:?}"))
            }
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
                TlsOutcome::CertificateHostMismatch(format!("{cert_err:?}"))
            }
            CertificateError::UnknownIssuer
            | CertificateError::BadSignature
            | CertificateError::BadEncoding
            | CertificateError::Revoked
            | CertificateError::InvalidPurpose => {
                if is_dane {
                    TlsOutcome::DaneValidationFailure(format!("{cert_err:?}"))
                } else {
                    TlsOutcome::CertificateNotTrusted(format!("{cert_err:?}"))
                }
            }
            _ => {
                // CertificateError is non-exhaustive; future variants
                // get the safe default.
                if is_dane {
                    TlsOutcome::DaneValidationFailure(format!("{cert_err:?}"))
                } else {
                    TlsOutcome::CertificateNotTrusted(format!("{cert_err:?}"))
                }
            }
        },
        rustls::Error::AlertReceived(alert) => {
            // BadCertificate / UnknownCA / CertificateExpired alerts
            // from the peer.
            use rustls::AlertDescription as A;
            match alert {
                A::CertificateExpired => TlsOutcome::CertificateExpired(format!("alert: {alert:?}")),
                A::BadCertificate
                | A::UnsupportedCertificate
                | A::CertificateRevoked
                | A::CertificateUnknown
                | A::UnknownCA => {
                    if is_dane {
                        TlsOutcome::DaneValidationFailure(format!("alert: {alert:?}"))
                    } else {
                        TlsOutcome::CertificateNotTrusted(format!("alert: {alert:?}"))
                    }
                }
                _ => TlsOutcome::Other(format!("alert: {alert:?}")),
            }
        }
        rustls::Error::General(s) if s.to_ascii_lowercase().contains("dane") => {
            TlsOutcome::DaneValidationFailure(s.clone())
        }
        _ => TlsOutcome::Other(format!("rustls: {e:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_returns_rfc_8460_strings() {
        assert_eq!(
            TlsOutcome::CertificateExpired("x".into()).as_str(),
            "certificate-expired"
        );
        assert_eq!(
            TlsOutcome::CertificateHostMismatch("x".into()).as_str(),
            "certificate-host-mismatch"
        );
        assert_eq!(
            TlsOutcome::CertificateNotTrusted("x".into()).as_str(),
            "certificate-not-trusted"
        );
        assert_eq!(
            TlsOutcome::DaneValidationFailure("x".into()).as_str(),
            "tlsa-invalid"
        );
        assert_eq!(
            TlsOutcome::InvalidServerName("x".into()).as_str(),
            "invalid-server-name"
        );
        assert_eq!(TlsOutcome::NetworkError("x".into()).as_str(), "network-error");
        assert_eq!(TlsOutcome::Other("x".into()).as_str(), "other");
    }

    #[test]
    fn detail_returns_inner_string() {
        let detail = "BadSignature";
        assert_eq!(
            TlsOutcome::CertificateExpired(detail.into()).detail(),
            detail
        );
        assert_eq!(
            TlsOutcome::DaneValidationFailure(detail.into()).detail(),
            detail
        );
    }

    #[test]
    fn classify_rustls_expired_maps_to_certificate_expired() {
        let e = rustls::Error::InvalidCertificate(rustls::CertificateError::Expired);
        assert_eq!(
            classify_rustls_error(&e, false),
            TlsOutcome::CertificateExpired("Expired".into())
        );
    }

    #[test]
    fn classify_rustls_not_valid_for_name_maps_to_host_mismatch() {
        let e = rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidForName);
        assert_eq!(
            classify_rustls_error(&e, false),
            TlsOutcome::CertificateHostMismatch("NotValidForName".into())
        );
    }

    #[test]
    fn classify_rustls_unknown_issuer_pkix_path() {
        let e = rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer);
        assert_eq!(
            classify_rustls_error(&e, false),
            TlsOutcome::CertificateNotTrusted("UnknownIssuer".into())
        );
    }

    #[test]
    fn classify_rustls_unknown_issuer_dane_path_marks_tlsa_invalid() {
        let e = rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer);
        assert_eq!(
            classify_rustls_error(&e, true),
            TlsOutcome::DaneValidationFailure("UnknownIssuer".into())
        );
    }

    #[test]
    fn classify_alert_expired() {
        let e = rustls::Error::AlertReceived(rustls::AlertDescription::CertificateExpired);
        assert!(matches!(
            classify_rustls_error(&e, false),
            TlsOutcome::CertificateExpired(_)
        ));
    }

    #[test]
    fn classify_general_dane_string_marks_dane() {
        let e = rustls::Error::General("DANE TLSA mismatch".into());
        assert!(matches!(
            classify_rustls_error(&e, false),
            TlsOutcome::DaneValidationFailure(_)
        ));
    }

    #[test]
    fn classify_io_timeout_becomes_network_error() {
        let e = io::Error::new(io::ErrorKind::TimedOut, "read timeout");
        assert!(matches!(
            classify_io_error(&e, false),
            TlsOutcome::NetworkError(_)
        ));
    }

    #[test]
    fn classify_io_unknown_kind_falls_back_to_other() {
        let e = io::Error::other("totally unknown");
        assert!(matches!(classify_io_error(&e, false), TlsOutcome::Other(_)));
    }
}

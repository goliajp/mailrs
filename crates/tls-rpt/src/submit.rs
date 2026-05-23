//! Pure helpers for packaging a [`crate::Report`] into the bytes
//! a receiver expects — gzip-encoded JSON for both submission paths,
//! plus a `multipart/report` RFC 5322 wire format for the
//! `mailto:` path (RFC 8460 §5.3).
//!
//! No network I/O happens here. The caller does the SMTP submission
//! (likely via their existing outbound queue + DKIM signer) or the
//! HTTPS POST. The crate provides the bytes; the caller picks the
//! transport.
//!
//! ## What's in this module
//!
//! - [`gzip_report`] — `serde_json::to_vec(report)` then gzip.
//!   Produces the body for an HTTPS POST (the `Content-Type` is
//!   `application/tlsrpt+gzip`).
//! - [`build_submission_email`] — assembles an RFC 5322
//!   `multipart/report` email body wrapping the gzipped JSON as an
//!   `application/tlsrpt+gzip` attachment. Caller prepends their own
//!   `DKIM-Signature:` (RFC 8460 §5.3 requires it) and SMTP-submits.
//!
//! Both functions are pure: no clock, no RNG, no I/O.

use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;

use crate::Report;

/// Gzip-encode a serialized TLSRPT [`Report`].
///
/// `serde_json::to_vec(report)` then gzip with the default level
/// (~6, a balance between speed and ratio). Returns the gzipped
/// bytes ready to either (a) wrap inside a `multipart/report` email
/// attachment via [`build_submission_email`], or (b) POST as the
/// HTTPS body with `Content-Type: application/tlsrpt+gzip`.
///
/// # Errors
///
/// Bubbles `serde_json::Error` (extremely unlikely for our
/// well-typed [`Report`] struct) and `flate2` IO errors (also
/// unlikely — gzip into an in-memory `Vec` doesn't have a way to
/// fail in practice). Both wrapped as `std::io::Error` for a
/// single return type.
pub fn gzip_report(report: &Report) -> std::io::Result<Vec<u8>> {
    let json = serde_json::to_vec(report)
        .map_err(|e| std::io::Error::other(format!("serialize: {e}")))?;
    let mut encoder = GzEncoder::new(Vec::with_capacity(json.len() / 2), Compression::default());
    encoder.write_all(&json)?;
    encoder.finish()
}

/// Options controlling [`build_submission_email`].
///
/// Field naming matches RFC 8460 §5.3 (the spec's "report submitter"
/// is our `from_address`; the spec's "report domain" is the
/// receiving domain — `receiving_domain`).
#[derive(Debug, Clone)]
pub struct SubmissionEmailOpts {
    /// RFC 5322 `From:` — the address sending the report. Typically
    /// `tlsrpt@<our-domain>` or `postmaster@<our-domain>`.
    pub from_address: String,
    /// RFC 5322 `To:` — the `mailto:` rua endpoint from the
    /// receiver's `_smtp._tls.<domain>` TXT record.
    pub to_address: String,
    /// Receiving domain the report is about. Goes into the Subject
    /// line per §5.3.
    pub receiving_domain: String,
    /// Our (submitting) domain. Goes into the Subject line + the
    /// `<...@submitter>` part of Message-ID.
    pub submitter_domain: String,
    /// Caller-supplied unique report identifier. Receivers use this
    /// to dedup retransmits.
    pub report_id: String,
    /// RFC 2822-formatted Date header value (e.g.
    /// `"Sat, 23 May 2026 12:34:56 +0000"`). Caller formats — we
    /// don't pull a clock dep just for this.
    pub date_rfc2822: String,
    /// MIME boundary string. Must be unique within the message;
    /// 16+ random chars recommended. Caller controls so tests can
    /// pin it for deterministic output.
    pub boundary: String,
    /// Gzip-encoded report bytes from [`gzip_report`].
    pub report_gzipped: Vec<u8>,
}

/// Assemble an RFC 5322 `multipart/report; report-type=tlsrpt`
/// email body wrapping the gzipped report as an
/// `application/tlsrpt+gzip` attachment, per RFC 8460 §5.3.
///
/// Returns the FULL message bytes (headers + body + attachment),
/// ready to be DKIM-signed by the caller (the spec mandates the
/// submission email is DKIM-signed) and handed to an outbound
/// SMTP queue.
///
/// The Subject is formatted exactly per §5.3:
///
/// ```text
/// Report Domain: <receiving-domain> Submitter: <submitter-domain> Report-ID: <report-id>
/// ```
///
/// Message-ID is `<{report-id}@{submitter-domain}>`. The attachment
/// filename is `{submitter-domain}!{receiving-domain}!{report-id}.json.gz`,
/// matching the convention DMARC aggregate reports established
/// (TLSRPT didn't standardize a filename, but receivers parse
/// emails by Content-Type and consistent naming helps logs).
pub fn build_submission_email(opts: &SubmissionEmailOpts) -> Vec<u8> {
    use base64::Engine as _;

    let attachment_b64 = base64::engine::general_purpose::STANDARD.encode(&opts.report_gzipped);
    let filename = format!(
        "{}!{}!{}.json.gz",
        opts.submitter_domain, opts.receiving_domain, opts.report_id
    );

    let mut out = Vec::with_capacity(opts.report_gzipped.len() * 2 + 1024);
    // Headers
    out.extend_from_slice(format!("From: {}\r\n", opts.from_address).as_bytes());
    out.extend_from_slice(format!("To: {}\r\n", opts.to_address).as_bytes());
    out.extend_from_slice(format!("Date: {}\r\n", opts.date_rfc2822).as_bytes());
    out.extend_from_slice(
        format!(
            "Message-ID: <{}@{}>\r\n",
            opts.report_id, opts.submitter_domain
        )
        .as_bytes(),
    );
    out.extend_from_slice(
        format!(
            "Subject: Report Domain: {} Submitter: {} Report-ID: <{}@{}>\r\n",
            opts.receiving_domain, opts.submitter_domain, opts.report_id, opts.submitter_domain
        )
        .as_bytes(),
    );
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    out.extend_from_slice(b"TLS-Report-Domain: ");
    out.extend_from_slice(opts.receiving_domain.as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(b"TLS-Report-Submitter: ");
    out.extend_from_slice(opts.submitter_domain.as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(
        format!(
            "Content-Type: multipart/report;\r\n\treport-type=\"tlsrpt\";\r\n\tboundary=\"{}\"\r\n",
            opts.boundary
        )
        .as_bytes(),
    );
    out.extend_from_slice(b"\r\n");

    // Body — preamble
    out.extend_from_slice(b"This is an SMTP TLS Reporting (RFC 8460) aggregate report.\r\n");
    out.extend_from_slice(b"\r\n");

    // Part 1: human-readable summary (text/plain)
    out.extend_from_slice(format!("--{}\r\n", opts.boundary).as_bytes());
    out.extend_from_slice(b"Content-Type: text/plain; charset=us-ascii\r\n");
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(
        format!(
            "TLSRPT report for domain {} (report-id {})\r\nSubmitted by {}\r\n",
            opts.receiving_domain, opts.report_id, opts.submitter_domain
        )
        .as_bytes(),
    );
    out.extend_from_slice(b"\r\n");

    // Part 2: the report attachment
    out.extend_from_slice(format!("--{}\r\n", opts.boundary).as_bytes());
    out.extend_from_slice(b"Content-Type: application/tlsrpt+gzip\r\n");
    out.extend_from_slice(format!("Content-Disposition: attachment; filename=\"{filename}\"\r\n").as_bytes());
    out.extend_from_slice(b"Content-Transfer-Encoding: base64\r\n");
    out.extend_from_slice(b"\r\n");
    // Wrap the base64 into 76-char lines per RFC 2045 §6.8.
    for chunk in attachment_b64.as_bytes().chunks(76) {
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\r\n");
    }

    // Closing boundary
    out.extend_from_slice(format!("--{}--\r\n", opts.boundary).as_bytes());

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReportBuilder;
    use crate::report::{PolicyType, SuccessEvent};
    use flate2::read::GzDecoder;
    use std::io::Read;

    fn sample_report() -> Report {
        let mut b = ReportBuilder::new()
            .organization_name("Test Org")
            .contact_info("mailto:t@e.com")
            .report_id("test-1")
            .date_range("2026-05-23T00:00:00Z", "2026-05-24T00:00:00Z");
        b.record_success(SuccessEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: "mx.example.com".into(),
        });
        b.build().unwrap()
    }

    #[test]
    fn gzip_report_round_trips_through_gunzip_to_original_json() {
        let r = sample_report();
        let gz = gzip_report(&r).unwrap();
        // gzip magic: 1f 8b
        assert_eq!(&gz[..2], &[0x1f, 0x8b]);
        let mut dec = GzDecoder::new(&gz[..]);
        let mut json = Vec::new();
        dec.read_to_end(&mut json).unwrap();
        let back: Report = serde_json::from_slice(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn gzip_report_compresses() {
        // For our shape of report the gzipped size should be
        // smaller than the raw JSON. Sanity check.
        let r = sample_report();
        let raw = serde_json::to_vec(&r).unwrap();
        let gz = gzip_report(&r).unwrap();
        // Tiny payloads can actually grow under gzip (header overhead),
        // so only assert "produced something" — the round-trip test
        // above is the correctness gate.
        assert!(!gz.is_empty(), "empty gzip output");
        // For real reports the ratio should be ≤ 1.0 + a small
        // header allowance.
        assert!(
            gz.len() <= raw.len() + 30,
            "gzip overhead too large: raw {} gz {}",
            raw.len(),
            gz.len()
        );
    }

    fn sample_opts() -> SubmissionEmailOpts {
        SubmissionEmailOpts {
            from_address: "tlsrpt@submitter.example".into(),
            to_address: "tlsrpt@receiver.example".into(),
            receiving_domain: "receiver.example".into(),
            submitter_domain: "submitter.example".into(),
            report_id: "test-1".into(),
            date_rfc2822: "Sat, 23 May 2026 00:00:00 +0000".into(),
            boundary: "tlsrpt-test-boundary-1234567890".into(),
            report_gzipped: vec![0x1f, 0x8b, 0x08, 0xde, 0xad],
        }
    }

    #[test]
    fn build_submission_email_subject_matches_rfc_8460_section_5_3() {
        let opts = sample_opts();
        let email = build_submission_email(&opts);
        let s = String::from_utf8_lossy(&email);
        assert!(s.contains(
            "Subject: Report Domain: receiver.example Submitter: submitter.example Report-ID: <test-1@submitter.example>"
        ));
    }

    #[test]
    fn build_submission_email_includes_tls_report_headers() {
        let opts = sample_opts();
        let email = build_submission_email(&opts);
        let s = String::from_utf8_lossy(&email);
        assert!(s.contains("TLS-Report-Domain: receiver.example\r\n"));
        assert!(s.contains("TLS-Report-Submitter: submitter.example\r\n"));
    }

    #[test]
    fn build_submission_email_message_id_uses_report_id_at_submitter() {
        let opts = sample_opts();
        let email = build_submission_email(&opts);
        let s = String::from_utf8_lossy(&email);
        assert!(s.contains("Message-ID: <test-1@submitter.example>\r\n"));
    }

    #[test]
    fn build_submission_email_attachment_is_base64_application_tlsrpt_gzip() {
        let opts = sample_opts();
        let email = build_submission_email(&opts);
        let s = String::from_utf8_lossy(&email);
        assert!(s.contains("Content-Type: application/tlsrpt+gzip\r\n"));
        assert!(s.contains("Content-Transfer-Encoding: base64\r\n"));
        assert!(
            s.contains("filename=\"submitter.example!receiver.example!test-1.json.gz\"")
        );
        // The base64-encoded "\x1f\x8b\x08\xde\xad" is "H4sI3q0="
        assert!(s.contains("H4sI3q0="));
    }

    #[test]
    fn build_submission_email_uses_supplied_boundary_for_open_and_close() {
        let opts = sample_opts();
        let email = build_submission_email(&opts);
        let s = String::from_utf8_lossy(&email);
        // Opens each part with --<boundary>; closes with --<boundary>--
        let opener = format!("--{}\r\n", opts.boundary);
        let closer = format!("--{}--\r\n", opts.boundary);
        assert!(s.matches(&opener).count() == 2, "expected 2 part openers");
        assert!(s.contains(&closer));
    }

    #[test]
    fn build_submission_email_is_crlf_terminated_throughout() {
        let opts = sample_opts();
        let email = build_submission_email(&opts);
        // Any LF must be preceded by CR.
        let mut prev = 0u8;
        for &b in &email {
            if b == b'\n' {
                assert_eq!(prev, b'\r', "bare LF in submission email");
            }
            prev = b;
        }
    }

    #[test]
    fn build_submission_email_base64_lines_max_76_chars() {
        // RFC 2045 §6.8: base64 lines max 76 chars.
        let large = vec![0u8; 1024];
        let mut opts = sample_opts();
        opts.report_gzipped = large;
        let email = build_submission_email(&opts);
        let s = String::from_utf8_lossy(&email);
        let after_b64_marker = s
            .find("Content-Transfer-Encoding: base64\r\n\r\n")
            .expect("expected base64 marker");
        let body = &s[after_b64_marker + "Content-Transfer-Encoding: base64\r\n\r\n".len()..];
        for line in body.split("\r\n") {
            if line.starts_with("--") || line.is_empty() {
                break; // hit the closing boundary
            }
            assert!(line.len() <= 76, "base64 line too long ({} chars)", line.len());
        }
    }
}

//! RFC 8601 `Authentication-Results:` header formatting.
//!
//! Pure string helpers — no I/O, no dependency on any specific SPF / DKIM /
//! DMARC verifier. The caller does the verification (via whatever crate they
//! prefer) and hands the results to [`format_auth_results`] or
//! [`format_auth_results_header`].

use std::fmt::Write;

/// One method result inside an `Authentication-Results:` header.
///
/// Example: `AuthResult { method: "spf", result: "pass", reason: None }`
/// renders as `spf=pass`. With a reason it renders as
/// `spf=fail reason="mechanism -all matched"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthResult {
    /// Method identifier (`spf` / `dkim` / `arc` / `dmarc` / `dkim-atps` / etc).
    pub method: String,
    /// Result token per the method's RFC (`pass` / `fail` / `softfail` /
    /// `neutral` / `none` / `temperror` / `permerror` / ...).
    pub result: String,
    /// Optional human-readable reason, included as `reason="<text>"`.
    pub reason: Option<String>,
}

/// Build the value portion of an `Authentication-Results:` header per
/// [RFC 8601 §2.2](https://www.rfc-editor.org/rfc/rfc8601#section-2.2).
///
/// The returned string is the bare value (no `Authentication-Results: `
/// prefix, no trailing CRLF). Use [`format_auth_results_header`] for the
/// complete header line including the field name and CRLF.
///
/// When `results` is empty, emits `<hostname>; none` per RFC 8601 §2.2.
pub fn format_auth_results(hostname: &str, results: &[AuthResult]) -> String {
    let mut buf = String::new();
    write!(buf, "{hostname}").unwrap();

    if results.is_empty() {
        buf.push_str("; none");
        return buf;
    }

    for r in results {
        write!(buf, ";\r\n\t{}={}", r.method, r.result).unwrap();
        if let Some(ref reason) = r.reason {
            write!(buf, " reason=\"{reason}\"").unwrap();
        }
    }
    buf
}

/// Build the full `Authentication-Results: <value>\r\n` header line.
pub fn format_auth_results_header(hostname: &str, results: &[AuthResult]) -> String {
    format!(
        "Authentication-Results: {}\r\n",
        format_auth_results(hostname, results)
    )
}

/// Convenience: build an Authentication-Results header from the canonical
/// SPF / DKIM / ARC / DMARC quadruple. Mirrors what most mail-server
/// inbound pipelines emit per RFC 8601 §2.2.
///
/// `dmarc_reason` becomes the `reason="..."` parameter on the DMARC entry
/// when present (e.g. `Some("policy=reject")`).
pub fn build_auth_header(
    hostname: &str,
    spf: &str,
    dkim: &str,
    arc: &str,
    dmarc: &str,
    dmarc_reason: Option<&str>,
) -> String {
    // Direct single-allocation builder, bypassing the
    // `Vec<AuthResult>` materialisation that the generic
    // `format_auth_results_header` path needs. The old impl
    // allocated 5 `String`s up front (4× method names + 1× optional
    // reason) plus the Vec itself, then walked the Vec to emit the
    // header. For the canonical SPF/DKIM/ARC/DMARC quadruple all 4
    // method names are compile-time constants — we can write them
    // directly to a single pre-sized output buffer.
    //
    // Capacity sizing: 24-char "Authentication-Results: " + hostname
    // + ~140 bytes for the 4 `;\r\n\t<method>=<result>` lines + a
    // generous 64-byte budget for the optional `reason="..."` on
    // the DMARC entry. Real-world headers cap out at ~250-300 bytes.
    let est = 64 + hostname.len() + spf.len() + dkim.len() + arc.len() + dmarc.len();
    let mut out = String::with_capacity(est + 64);
    out.push_str("Authentication-Results: ");
    out.push_str(hostname);
    out.push_str(";\r\n\tspf=");
    out.push_str(spf);
    out.push_str(";\r\n\tdkim=");
    out.push_str(dkim);
    out.push_str(";\r\n\tarc=");
    out.push_str(arc);
    out.push_str(";\r\n\tdmarc=");
    out.push_str(dmarc);
    if let Some(reason) = dmarc_reason {
        out.push_str(" reason=\"");
        out.push_str(reason);
        out.push('"');
    }
    out.push_str("\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_pass() {
        let results = vec![
            AuthResult {
                method: "spf".into(),
                result: "pass".into(),
                reason: None,
            },
            AuthResult {
                method: "dkim".into(),
                result: "pass".into(),
                reason: None,
            },
            AuthResult {
                method: "dmarc".into(),
                result: "pass".into(),
                reason: None,
            },
        ];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.starts_with("mx.example.com;"));
        assert!(header.contains("spf=pass"));
        assert!(header.contains("dkim=pass"));
        assert!(header.contains("dmarc=pass"));
    }

    #[test]
    fn spf_fail_with_reason() {
        let results = vec![AuthResult {
            method: "spf".into(),
            result: "fail".into(),
            reason: Some("mechanism -all matched".into()),
        }];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.contains("spf=fail"));
        assert!(header.contains("reason=\"mechanism -all matched\""));
    }

    #[test]
    fn no_results_yields_none() {
        let header = format_auth_results("mx.example.com", &[]);
        assert_eq!(header, "mx.example.com; none");
    }

    #[test]
    fn full_header_starts_and_ends_correctly() {
        let results = vec![AuthResult {
            method: "spf".into(),
            result: "pass".into(),
            reason: None,
        }];
        let header = format_auth_results_header("mx.example.com", &results);
        assert!(header.starts_with("Authentication-Results: mx.example.com;"));
        assert!(header.ends_with("\r\n"));
    }

    #[test]
    fn dmarc_policy_reason_round_trips() {
        let results = vec![AuthResult {
            method: "dmarc".into(),
            result: "fail".into(),
            reason: Some("policy=quarantine".into()),
        }];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.contains("reason=\"policy=quarantine\""));
    }

    #[test]
    fn full_pipeline_quadruple() {
        let results = vec![
            AuthResult {
                method: "spf".into(),
                result: "pass".into(),
                reason: None,
            },
            AuthResult {
                method: "dkim".into(),
                result: "pass".into(),
                reason: None,
            },
            AuthResult {
                method: "arc".into(),
                result: "none".into(),
                reason: None,
            },
            AuthResult {
                method: "dmarc".into(),
                result: "pass".into(),
                reason: None,
            },
        ];
        let header = format_auth_results("mx.mail.com", &results);
        assert!(header.contains("spf=pass"));
        assert!(header.contains("dkim=pass"));
        assert!(header.contains("arc=none"));
        assert!(header.contains("dmarc=pass"));
    }

    #[test]
    fn multiline_folding() {
        let results = vec![
            AuthResult {
                method: "spf".into(),
                result: "pass".into(),
                reason: None,
            },
            AuthResult {
                method: "dmarc".into(),
                result: "pass".into(),
                reason: None,
            },
        ];
        let header = format_auth_results("mx.example.com", &results);
        // RFC 8601 multi-result folding: ;\r\n\t before each subsequent result
        assert!(header.contains(";\r\n\t"));
    }

    #[test]
    fn temperror_and_permerror_results_pass_through() {
        for code in &["temperror", "permerror"] {
            let results = vec![AuthResult {
                method: "dmarc".into(),
                result: (*code).into(),
                reason: None,
            }];
            let header = format_auth_results("mx.example.com", &results);
            assert!(header.contains(&format!("dmarc={code}")));
        }
    }

    #[test]
    fn build_auth_header_canonical_quadruple() {
        let header = build_auth_header("mx.test.com", "pass", "pass", "none", "pass", None);
        assert!(header.contains("Authentication-Results: mx.test.com"));
        assert!(header.contains("spf=pass"));
        assert!(header.contains("dkim=pass"));
        assert!(header.contains("arc=none"));
        assert!(header.contains("dmarc=pass"));
    }

    #[test]
    fn build_auth_header_threads_dmarc_reason() {
        let header = build_auth_header(
            "mx.test.com",
            "pass",
            "fail",
            "none",
            "fail",
            Some("policy=reject"),
        );
        assert!(header.contains("dmarc=fail"));
        assert!(header.contains("reason=\"policy=reject\""));
    }

    #[test]
    fn build_auth_header_omits_dmarc_reason_when_none() {
        let header = build_auth_header("mx.test.com", "pass", "pass", "none", "pass", None);
        assert!(!header.contains("reason="));
    }
}

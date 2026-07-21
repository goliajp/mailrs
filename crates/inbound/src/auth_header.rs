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

// ── parse side ──────────────────────────────────────────────────────
//
// The receive pipeline formats results into the header. When mail is
// read back (fastcore ingest), the header is all that survives — so we
// parse it back into structured method/result pairs and fold those into
// a sender-trust verdict. This is the self-hosted "is this sender who
// they claim to be" signal: pure cryptographic auth results, no model.

/// Parse the value of one `Authentication-Results:` field (the part
/// after `Authentication-Results:`, unfolded) into its method results.
/// The leading authserv-id and any trailing comment are ignored; each
/// `method=result` token becomes an [`AuthResult`]. Robust to the
/// folding whitespace and `reason="..."` / `(comment)` decorations real
/// mail carries.
pub fn parse_auth_results(value: &str) -> Vec<AuthResult> {
    // Unfold, then split on ';' — the authserv-id is the first segment
    // and carries no '=', so it drops out naturally.
    let flat = value.replace(['\r', '\n', '\t'], " ");
    let mut out = Vec::new();
    for seg in flat.split(';') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        // A method segment is `method=result [k=v | (comment) | reason=".."]`.
        // Take the first `token=token` as the method/result; ignore the rest.
        let Some((method, rest)) = seg.split_once('=') else {
            continue;
        };
        let method = method.trim();
        // method must be a bare keyword (spf/dkim/dmarc/arc/...), not a
        // property like `header.from` that appears later in the segment.
        if method.is_empty()
            || !method
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            continue;
        }
        let result = rest.split_whitespace().next().unwrap_or("").to_string();
        if result.is_empty() {
            continue;
        }
        out.push(AuthResult {
            method: method.to_ascii_lowercase(),
            result: result.to_ascii_lowercase(),
            reason: None,
        });
    }
    out
}

/// How much the receive-time authentication vouches for the sender
/// being who the envelope claims. Ordered least → most alarming is not
/// the intent; treat these as distinct states, not a scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenderTrust {
    /// DMARC passed — the From domain is aligned and SPF or DKIM
    /// verified it. As strong as email authentication gets.
    Verified,
    /// Authentication ran but did not reach DMARC pass: no DMARC record,
    /// only SPF/DKIM with no alignment, or results absent. Not proof of
    /// anything either way — the ordinary state for a lot of real mail.
    Unverified,
    /// An authentication method actively failed — DMARC fail, or SPF
    /// fail / DKIM fail with nothing passing. The From domain is being
    /// spoofed, or the sender's own setup is broken. Worth flagging.
    Suspicious,
}

/// Fold parsed method results into a [`SenderTrust`] verdict.
///
/// DMARC is authoritative when present, because it is the method that
/// ties the visible From domain to a passing SPF or DKIM check — which
/// is exactly the spoofing question. Only when DMARC is absent do we
/// fall back to the weaker signal of a raw SPF/DKIM failure.
pub fn sender_trust(results: &[AuthResult]) -> SenderTrust {
    let find = |m: &str| {
        results
            .iter()
            .find(|r| r.method == m)
            .map(|r| r.result.as_str())
    };

    match find("dmarc") {
        Some("pass") => return SenderTrust::Verified,
        Some("fail") => return SenderTrust::Suspicious,
        _ => {}
    }
    // No usable DMARC verdict. A hard SPF or DKIM fail with nothing
    // passing still points at spoofing/misconfiguration.
    let spf = find("spf");
    let dkim = find("dkim");
    let any_pass = spf == Some("pass") || dkim == Some("pass");
    let any_fail = spf == Some("fail") || dkim == Some("fail");
    match (any_fail, any_pass) {
        (true, false) => SenderTrust::Suspicious,
        _ => SenderTrust::Unverified,
    }
}

/// String form for storage / wire — stable tokens, not for display.
impl SenderTrust {
    /// Stable lowercase token for persistence and the wire
    /// (`"verified"` / `"unverified"` / `"suspicious"`).
    pub fn as_str(self) -> &'static str {
        match self {
            SenderTrust::Verified => "verified",
            SenderTrust::Unverified => "unverified",
            SenderTrust::Suspicious => "suspicious",
        }
    }
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

    // ── parse + trust ──

    #[test]
    fn round_trips_our_own_header() {
        let header = build_auth_header("mx.test.com", "pass", "pass", "none", "pass", None);
        // strip the field name our formatter prepends
        let value = header.trim_start_matches("Authentication-Results:");
        let parsed = parse_auth_results(value);
        let get = |m: &str| {
            parsed
                .iter()
                .find(|r| r.method == m)
                .map(|r| r.result.as_str())
        };
        assert_eq!(get("spf"), Some("pass"));
        assert_eq!(get("dkim"), Some("pass"));
        assert_eq!(get("dmarc"), Some("pass"));
    }

    #[test]
    fn parses_a_real_gmail_style_header() {
        // The shape Gmail / Outlook actually emit: authserv-id, folded
        // whitespace, header.i / header.from properties, comments.
        let v = "mx.google.com;\r\n\
                 dkim=pass header.i=@example.com header.s=sel;\r\n\
                 spf=pass (google.com: domain of a@example.com designates 1.2.3.4) smtp.mailfrom=a@example.com;\r\n\
                 dmarc=pass (p=REJECT sp=REJECT dis=NONE) header.from=example.com";
        let parsed = parse_auth_results(v);
        let get = |m: &str| {
            parsed
                .iter()
                .find(|r| r.method == m)
                .map(|r| r.result.as_str())
        };
        assert_eq!(get("dkim"), Some("pass"));
        assert_eq!(get("spf"), Some("pass"));
        assert_eq!(get("dmarc"), Some("pass"));
        // property tokens like header.from must NOT be read as methods
        assert!(parsed.iter().all(|r| r.method != "header.from"));
    }

    #[test]
    fn dmarc_pass_is_verified() {
        let r = parse_auth_results("mx; spf=fail; dkim=pass; dmarc=pass");
        assert_eq!(sender_trust(&r), SenderTrust::Verified);
    }

    #[test]
    fn dmarc_fail_is_suspicious_even_if_spf_passes() {
        // classic spoof: envelope passes SPF for the attacker's domain,
        // but the visible From fails DMARC alignment.
        let r = parse_auth_results("mx; spf=pass; dmarc=fail");
        assert_eq!(sender_trust(&r), SenderTrust::Suspicious);
    }

    #[test]
    fn no_dmarc_hard_spf_fail_is_suspicious() {
        let r = parse_auth_results("mx; spf=fail; dkim=none");
        assert_eq!(sender_trust(&r), SenderTrust::Suspicious);
    }

    #[test]
    fn no_dmarc_and_nothing_conclusive_is_unverified() {
        let r = parse_auth_results("mx; spf=none; dkim=none");
        assert_eq!(sender_trust(&r), SenderTrust::Unverified);
    }

    #[test]
    fn empty_or_none_header_is_unverified_not_a_panic() {
        assert_eq!(
            sender_trust(&parse_auth_results("mx; none")),
            SenderTrust::Unverified
        );
        assert_eq!(
            sender_trust(&parse_auth_results("")),
            SenderTrust::Unverified
        );
    }
}

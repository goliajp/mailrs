//! Receive-side request context + signal accumulator.
//!
//! [`ReceiveContext`] is the state object every pipeline stage reads from
//! and writes into. It carries the static request data (client IP, envelope
//! addresses, message body) plus the running signal totals each stage
//! contributes to.

use std::net::IpAddr;

use crate::decision::PipelineInput;

/// Aggregate state for one SMTP receive transaction.
///
/// Constructed at the start of the pipeline from the SMTP envelope + DATA
/// body. Stages take `&mut ReceiveContext` and mutate the signal fields
/// (`auth_results`, `virus_found`, `content_score`, etc.) as they execute.
/// At the end, the [`Pipeline`](crate::Pipeline) executor materializes a
/// [`PipelineInput`] from the accumulated state and calls
/// [`make_delivery_decision`](crate::make_delivery_decision).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReceiveContext {
    // ===== Static request data =====
    /// Client IP that connected and submitted the message.
    pub client_ip: IpAddr,
    /// Domain the client claimed in EHLO/HELO.
    pub ehlo_domain: String,
    /// Envelope MAIL FROM (reverse path).
    pub sender: String,
    /// Envelope RCPT TO (first / primary recipient).
    pub recipient: String,
    /// Raw RFC 5322 message body the client transmitted in DATA.
    pub message: Vec<u8>,
    /// This server's own hostname (used in the Authentication-Results
    /// header's `authserv-id`).
    pub hostname: String,

    // ===== Signal accumulators — written by stages =====
    /// SPF / DKIM / ARC / DMARC verification summary. Stages typically fill
    /// `auth_results.spf` first, then `dkim`, etc.
    pub auth_results: AuthResults,
    /// `true` when a greylisting stage decided to defer.
    pub greylisted: bool,
    /// `Some(signature_name)` when a virus scanner found malware.
    pub virus_found: Option<String>,
    /// Rule-engine content score (higher = spammier).
    pub content_score: f64,
    /// Names of rules that fired. Surfaced in the Junk decision's reason.
    pub matched_rules: Vec<String>,
    /// FCrDNS score from a PTR-check stage.
    pub ptr_score: f64,
    /// Score from an AI / ML scoring stage.
    pub ai_score: f64,

    // ===== v2.4.1 Phase 3 (RFC-B) — sender allow / block =====
    /// Envelope `From:` address (lowercased) for whitelist / blacklist
    /// lookup. The caller (fastcore inbound handler) sets this before
    /// running the pipeline; stages don't touch it.
    pub from_addr: String,
    /// Recipient's per-user whitelist. Populated by the caller with a
    /// snapshot of `spam:{user}:whitelist` for `recipient`.
    pub recipient_whitelist: std::collections::HashSet<String>,
    /// Recipient's per-user blacklist. Populated by the caller with a
    /// snapshot of `spam:{user}:blacklist` for `recipient`.
    pub recipient_blacklist: std::collections::HashSet<String>,
}

impl ReceiveContext {
    /// Construct a fresh context for one receive transaction. All signal
    /// fields start zeroed; stages fill them in as the pipeline runs.
    pub fn new(
        client_ip: IpAddr,
        ehlo_domain: impl Into<String>,
        sender: impl Into<String>,
        recipient: impl Into<String>,
        message: Vec<u8>,
        hostname: impl Into<String>,
    ) -> Self {
        Self {
            client_ip,
            ehlo_domain: ehlo_domain.into(),
            sender: sender.into(),
            recipient: recipient.into(),
            message,
            hostname: hostname.into(),
            auth_results: AuthResults::default(),
            greylisted: false,
            virus_found: None,
            content_score: 0.0,
            matched_rules: Vec::new(),
            ptr_score: 0.0,
            ai_score: 0.0,
            from_addr: String::new(),
            recipient_whitelist: std::collections::HashSet::new(),
            recipient_blacklist: std::collections::HashSet::new(),
        }
    }

    /// Materialize a [`PipelineInput`] from the accumulated signals, ready
    /// to hand to [`make_delivery_decision`](crate::make_delivery_decision).
    pub fn to_pipeline_input(&self, spam_threshold: f64) -> PipelineInput {
        PipelineInput {
            greylisted: self.greylisted,
            auth: self.auth_results.clone(),
            virus_found: self.virus_found.clone(),
            content_score: self.content_score,
            matched_rules: self.matched_rules.clone(),
            ptr_score: self.ptr_score,
            ai_score: self.ai_score,
            spam_threshold,
            hostname: self.hostname.clone(),
            from_addr: self.from_addr.clone(),
            recipient_whitelist: self.recipient_whitelist.clone(),
            recipient_blacklist: self.recipient_blacklist.clone(),
        }
    }
}

/// SPF / DKIM / ARC / DMARC verification summary.
///
/// Each result is a free-form lowercase token per the relevant RFC
/// (`pass` / `fail` / `softfail` / `neutral` / `none` / `temperror` /
/// `permerror`). The token shape matches what
/// [`crate::auth_header::build_auth_header`] expects.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthResults {
    /// SPF result (RFC 7208).
    pub spf: String,
    /// DKIM result (RFC 6376) — aggregated across all signatures: `pass` if
    /// any signature verified, else `fail` (when any signature was present
    /// and failed), else `none`.
    pub dkim: String,
    /// ARC result (RFC 8617): `pass` / `fail` / `none`.
    pub arc: String,
    /// DMARC result (RFC 7489): `pass` / `fail` / `none` / `temperror`.
    pub dmarc: String,
    /// The DMARC policy advertised by the sending domain (`p=` tag),
    /// used to gate the [`make_delivery_decision`](crate::make_delivery_decision)
    /// outcome.
    pub dmarc_policy: DmarcPolicy,
}

impl AuthResults {
    /// Receive-time sender-trust verdict, folded from the structured
    /// SPF/DKIM/DMARC tokens this transaction produced.
    ///
    /// Delegates to [`fold_sender_trust`](crate::auth_header::fold_sender_trust)
    /// — the same core the header-parse path uses — so the two never
    /// diverge. The tokens are already lowercase RFC keywords
    /// (`pass`/`fail`/`none`/...); non-`pass`/`fail` values fold to
    /// `Unverified` naturally.
    pub fn sender_trust(&self) -> crate::auth_header::SenderTrust {
        crate::auth_header::fold_sender_trust(
            Some(self.dmarc.as_str()),
            Some(self.spf.as_str()),
            Some(self.dkim.as_str()),
        )
    }
}

impl Default for AuthResults {
    fn default() -> Self {
        Self {
            spf: "none".into(),
            dkim: "none".into(),
            arc: "none".into(),
            dmarc: "none".into(),
            dmarc_policy: DmarcPolicy::None,
        }
    }
}

/// DMARC policy advertised by the From-domain's `p=` tag (RFC 7489 §6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmarcPolicy {
    /// `p=reject` — sender requests outright rejection on alignment failure.
    Reject,
    /// `p=quarantine` — sender requests delivery to Junk on alignment failure.
    Quarantine,
    /// `p=none` — sender requests monitoring only (still deliver).
    None,
    /// DMARC verification passed; no policy enforcement needed.
    Pass,
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    fn ctx() -> ReceiveContext {
        ReceiveContext::new(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            "client.example.com",
            "alice@example.com",
            "bob@example.com",
            b"From: alice\r\n\r\nhello".to_vec(),
            "mx.example.com",
        )
    }

    #[test]
    fn new_zeroes_all_signal_fields() {
        let c = ctx();
        assert!(!c.greylisted);
        assert!(c.virus_found.is_none());
        assert_eq!(c.content_score, 0.0);
        assert!(c.matched_rules.is_empty());
        assert_eq!(c.ptr_score, 0.0);
        assert_eq!(c.ai_score, 0.0);
    }

    #[test]
    fn auth_results_default_is_none() {
        let a = AuthResults::default();
        assert_eq!(a.spf, "none");
        assert_eq!(a.dkim, "none");
        assert_eq!(a.arc, "none");
        assert_eq!(a.dmarc, "none");
        assert_eq!(a.dmarc_policy, DmarcPolicy::None);
    }

    // Contract: the structured `AuthResults::sender_trust()` fold must
    // agree with the header-parse fold on the same SPF/DKIM/DMARC tokens.
    // Both delegate to `fold_sender_trust`; this pins that they can't
    // drift (feedback-two-impls-need-a-contract-test).
    #[test]
    fn structured_and_header_folds_agree() {
        use crate::auth_header::{build_auth_header, parse_auth_results, sender_trust};

        // (spf, dkim, dmarc) triples covering each verdict.
        let cases = [
            ("pass", "pass", "pass"), // Verified
            ("pass", "none", "fail"), // Suspicious (classic spoof: SPF ok, DMARC fail)
            ("fail", "none", "none"), // Suspicious (hard SPF fail, no DMARC)
            ("none", "none", "none"), // Unverified
            ("pass", "fail", "none"), // Unverified (one pass, one fail, no DMARC)
        ];
        for (spf, dkim, dmarc) in cases {
            let structured = AuthResults {
                spf: spf.into(),
                dkim: dkim.into(),
                arc: "none".into(),
                dmarc: dmarc.into(),
                dmarc_policy: DmarcPolicy::None,
            };
            // Round-trip through the header the receive pipeline emits.
            let header = build_auth_header("mx.test", spf, dkim, "none", dmarc, None);
            let value = header.trim_start_matches("Authentication-Results:");
            let from_header = sender_trust(&parse_auth_results(value));

            assert_eq!(
                structured.sender_trust(),
                from_header,
                "folds disagreed for spf={spf} dkim={dkim} dmarc={dmarc}"
            );
        }
    }

    #[test]
    fn to_pipeline_input_carries_threshold_and_hostname() {
        let c = ctx();
        let input = c.to_pipeline_input(7.5);
        assert_eq!(input.spam_threshold, 7.5);
        assert_eq!(input.hostname, "mx.example.com");
    }

    #[test]
    fn to_pipeline_input_round_trips_signals() {
        let mut c = ctx();
        c.greylisted = true;
        c.virus_found = Some("X".into());
        c.content_score = 1.5;
        c.matched_rules.push("rule-a".into());
        c.ptr_score = 0.5;
        c.ai_score = 2.0;
        let input = c.to_pipeline_input(5.0);
        assert!(input.greylisted);
        assert_eq!(input.virus_found.as_deref(), Some("X"));
        assert_eq!(input.content_score, 1.5);
        assert_eq!(input.matched_rules, vec!["rule-a"]);
        assert_eq!(input.ptr_score, 0.5);
        assert_eq!(input.ai_score, 2.0);
    }

    // ===== Additional corner-case tests =====

    #[test]
    fn clone_preserves_all_signal_state() {
        // ReceiveContext is Clone; verify deep-copy of all mutable signals.
        let mut c = ctx();
        c.content_score = 4.2;
        c.matched_rules.push("foo".into());
        c.ai_score = 1.1;
        let cloned = c.clone();
        assert_eq!(cloned.content_score, 4.2);
        assert_eq!(cloned.matched_rules, vec!["foo".to_string()]);
        assert_eq!(cloned.ai_score, 1.1);
        // mutating the clone doesn't affect the original
        let mut cloned = cloned;
        cloned.content_score = 0.0;
        assert_eq!(c.content_score, 4.2);
    }

    #[test]
    fn to_pipeline_input_clones_matched_rules() {
        // The input must own its matched_rules — mutating the context after
        // generating the input should not affect the input.
        let mut c = ctx();
        c.matched_rules.push("orig".into());
        let input = c.to_pipeline_input(5.0);
        c.matched_rules.push("post".into());
        assert_eq!(input.matched_rules, vec!["orig"]);
    }

    #[test]
    fn auth_results_clone_independence() {
        // Cloning AuthResults must produce an independent copy.
        let mut a = AuthResults {
            spf: "pass".into(),
            ..AuthResults::default()
        };
        let b = a.clone();
        a.spf = "fail".into();
        assert_eq!(b.spf, "pass");
    }
}

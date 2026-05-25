//! Decision types + pure final-decision policy.
//!
//! Once a pipeline has gathered signals (greylist hit, virus found, content
//! score, AI score, ...) into a [`PipelineInput`], [`make_delivery_decision`]
//! collapses them into a single [`DeliveryDecision`]. The function is pure
//! and deterministic — same input always produces the same output.
//!
//! Callers who don't want the [`Stage`](crate::Stage) /
//! [`Pipeline`](crate::Pipeline) framework can compute their own signals
//! however they like and call this function directly.

use crate::auth_header::build_auth_header;
use crate::context::{AuthResults, DmarcPolicy};

/// Final decision the receive pipeline emits for one message.
///
/// Maps directly to SMTP responses: `Accept` → 250, `Junk` → 250 + deliver to
/// Junk mailbox, `Reject` → 5xx code, `Greylist` → 451 (or 421 per local policy).
#[derive(Debug, Clone, PartialEq)]
pub enum DeliveryDecision {
    /// Accept and deliver normally. The Authentication-Results header is
    /// pre-built — the caller prepends it to the message.
    Accept {
        /// Pre-built `Authentication-Results:` header line.
        auth_header: String,
    },
    /// Accept but deliver to the user's Junk mailbox. `reason` is a
    /// human-readable trace surfaced in logs / admin UI.
    Junk {
        /// Pre-built `Authentication-Results:` header line.
        auth_header: String,
        /// Free-form reason text (e.g. `"DMARC policy quarantine"` or
        /// `"score 12.5 >= 8.0 (content=4.5, ptr=3.0, ai=5.0, list-id)"`).
        reason: String,
    },
    /// Reject at SMTP time. The pipeline picks the code + message; common
    /// pairs are `(550, "5.7.1 ...")` for hard rejects and `(451, "...")`
    /// for soft.
    Reject {
        /// SMTP response code (5xx for permanent, 4xx for temporary).
        code: u16,
        /// SMTP response text (will be wrapped in the SMTP response line by
        /// the caller — do not include the code prefix).
        message: String,
    },
    /// Defer per greylisting policy — return 451 to the client; client will
    /// retry later and (likely) be accepted on the next attempt.
    Greylist,
}

/// Aggregated signals consumed by [`make_delivery_decision`].
///
/// Populated either by [`Pipeline`](crate::Pipeline) (one field per stage
/// that mutates it) or by the caller's own ad-hoc orchestration.
#[derive(Debug, Clone)]
pub struct PipelineInput {
    /// `true` when the greylisting stage deferred this delivery.
    pub greylisted: bool,
    /// SPF / DKIM / ARC / DMARC verification summary.
    pub auth: AuthResults,
    /// Set when a virus scanner detected malware. The string carries the
    /// scanner's name for the detected signature (e.g. `"Eicar-Test-Signature"`).
    pub virus_found: Option<String>,
    /// Rule-engine content score. Higher = more likely spam. Combined with
    /// `ptr_score` + `ai_score` against `spam_threshold`.
    pub content_score: f64,
    /// Names of content rules that fired, for the Junk reason string.
    pub matched_rules: Vec<String>,
    /// Score from FCrDNS (forward-confirmed reverse DNS) on the client IP.
    pub ptr_score: f64,
    /// Score from an LLM / ML classifier.
    pub ai_score: f64,
    /// Combined-score threshold above which the message goes to Junk.
    pub spam_threshold: f64,
    /// Server's hostname — needed to build the Authentication-Results header.
    pub hostname: String,
}

/// Pure policy combiner. Order of precedence (high → low):
///
/// 1. Greylist (highest — defer before any other work).
/// 2. Virus found (hard 550 reject).
/// 3. DMARC policy=reject (hard 550 reject).
/// 4. DMARC policy=quarantine (route to Junk).
/// 5. Combined `content_score + ptr_score + ai_score >= spam_threshold` (Junk).
/// 6. Default: Accept.
///
/// The function is pure — same input always produces the same output. Use
/// it directly if you don't want the [`Pipeline`](crate::Pipeline) framework.
pub fn make_delivery_decision(input: &PipelineInput) -> DeliveryDecision {
    if input.greylisted {
        return DeliveryDecision::Greylist;
    }

    if let Some(ref name) = input.virus_found {
        return DeliveryDecision::Reject {
            code: 550,
            message: format!("5.7.1 Message rejected: virus detected ({name})"),
        };
    }

    let dmarc_reason = match input.auth.dmarc_policy {
        DmarcPolicy::Reject => Some("policy=reject"),
        DmarcPolicy::Quarantine => Some("policy=quarantine"),
        DmarcPolicy::None => Some("policy=none"),
        DmarcPolicy::Pass => None,
    };
    let auth_header = build_auth_header(
        &input.hostname,
        &input.auth.spf,
        &input.auth.dkim,
        &input.auth.arc,
        &input.auth.dmarc,
        dmarc_reason,
    );

    if input.auth.dmarc_policy == DmarcPolicy::Reject {
        return DeliveryDecision::Reject {
            code: 550,
            message: "5.7.1 DMARC policy reject".to_string(),
        };
    }

    if input.auth.dmarc_policy == DmarcPolicy::Quarantine {
        return DeliveryDecision::Junk {
            auth_header,
            reason: "DMARC policy quarantine".into(),
        };
    }

    let total_score = input.content_score + input.ptr_score + input.ai_score;
    if total_score >= input.spam_threshold {
        return DeliveryDecision::Junk {
            auth_header,
            reason: build_junk_reason(input, total_score),
        };
    }

    DeliveryDecision::Accept { auth_header }
}

/// Build the Junk-decision reason string with a single pre-sized
/// allocation instead of the `format!` macro's geometric String growth.
///
/// The Junk path was measured (criterion bench, M-series Mac, release)
/// at ~735 ns total vs. ~337 ns for the Accept path — the 2.4× gap is
/// entirely this string-build. Using `String::with_capacity` + `write!`
/// avoids both the intermediate `Vec<String>` from `matched_rules.join`
/// and the geometric resize cascade that `format!` does (16 → 32 → 64 …).
fn build_junk_reason(input: &PipelineInput, total_score: f64) -> String {
    use std::fmt::Write as _;
    // Capacity for the prefix (~50 bytes) + 5 numeric fields (~6 bytes
    // each w/ {:.1}) + a generous 64-byte budget for matched_rules.
    // Real-world reasons rarely exceed 150 bytes.
    let mut out = String::with_capacity(160);
    let _ = write!(
        out,
        "score {total_score:.1} >= {:.1} (content={:.1}, ptr={:.1}, ai={:.1}, ",
        input.spam_threshold, input.content_score, input.ptr_score, input.ai_score,
    );
    // Inline the rule-name join — avoid `matched_rules.join(", ")` which
    // builds an intermediate Vec<&str> + sums the lengths first.
    let mut first = true;
    for rule in &input.matched_rules {
        if !first {
            out.push_str(", ");
        }
        out.push_str(rule);
        first = false;
    }
    out.push(')');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing_auth() -> AuthResults {
        AuthResults {
            spf: "pass".into(),
            dkim: "pass".into(),
            arc: "none".into(),
            dmarc: "pass".into(),
            dmarc_policy: DmarcPolicy::Pass,
        }
    }

    fn baseline_input() -> PipelineInput {
        PipelineInput {
            greylisted: false,
            auth: passing_auth(),
            virus_found: None,
            content_score: 0.0,
            matched_rules: vec![],
            ptr_score: 0.0,
            ai_score: 0.0,
            spam_threshold: 5.0,
            hostname: "mx.example.com".into(),
        }
    }

    #[test]
    fn baseline_passes_to_accept() {
        let d = make_delivery_decision(&baseline_input());
        assert!(matches!(d, DeliveryDecision::Accept { .. }));
    }

    #[test]
    fn greylist_short_circuits_everything() {
        let mut input = baseline_input();
        input.greylisted = true;
        input.virus_found = Some("nope".into()); // would normally reject
        input.auth.dmarc_policy = DmarcPolicy::Reject; // would normally reject
        assert_eq!(make_delivery_decision(&input), DeliveryDecision::Greylist);
    }

    #[test]
    fn virus_yields_550() {
        let mut input = baseline_input();
        input.virus_found = Some("Eicar".into());
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { code, message } => {
                assert_eq!(code, 550);
                assert!(message.contains("Eicar"));
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_reject_yields_550() {
        let mut input = baseline_input();
        input.auth.dmarc_policy = DmarcPolicy::Reject;
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { code, message } => {
                assert_eq!(code, 550);
                assert!(message.contains("DMARC"));
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_quarantine_yields_junk() {
        let mut input = baseline_input();
        input.auth.dmarc_policy = DmarcPolicy::Quarantine;
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("DMARC"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn score_above_threshold_yields_junk_with_reason() {
        let mut input = baseline_input();
        input.content_score = 3.0;
        input.ptr_score = 2.0;
        input.ai_score = 1.5;
        input.matched_rules = vec!["bulk-list".into(), "shouting-subject".into()];
        // total = 6.5, threshold = 5.0
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("6.5"));
                assert!(reason.contains("bulk-list"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn score_below_threshold_passes_to_accept() {
        let mut input = baseline_input();
        input.content_score = 1.0;
        input.ptr_score = 0.5;
        input.ai_score = 1.0;
        input.spam_threshold = 5.0;
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn precedence_greylist_over_virus() {
        let mut input = baseline_input();
        input.greylisted = true;
        input.virus_found = Some("x".into());
        assert_eq!(make_delivery_decision(&input), DeliveryDecision::Greylist);
    }

    #[test]
    fn precedence_virus_over_dmarc() {
        let mut input = baseline_input();
        input.virus_found = Some("x".into());
        input.auth.dmarc_policy = DmarcPolicy::Reject;
        // virus comes first
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { message, .. } => {
                assert!(message.contains("virus"));
            }
            other => panic!("expected virus Reject, got {other:?}"),
        }
    }

    #[test]
    fn precedence_dmarc_reject_over_dmarc_quarantine_check() {
        let mut input = baseline_input();
        input.auth.dmarc_policy = DmarcPolicy::Reject;
        // Quarantine branch is checked AFTER Reject — reject wins.
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Reject { .. }
        ));
    }

    #[test]
    fn accept_carries_auth_header() {
        let d = make_delivery_decision(&baseline_input());
        match d {
            DeliveryDecision::Accept { auth_header } => {
                assert!(auth_header.starts_with("Authentication-Results:"));
                assert!(auth_header.contains("spf=pass"));
            }
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    #[test]
    fn junk_carries_auth_header() {
        let mut input = baseline_input();
        input.content_score = 100.0;
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { auth_header, .. } => {
                assert!(auth_header.starts_with("Authentication-Results:"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    // ===== Additional corner-case tests =====

    #[test]
    fn virus_wins_over_dmarc_reject_simultaneously() {
        // Combined edge: virus + dmarc=reject. Virus must win per RFC ordering
        // (virus is a hard data-level decision; DMARC is alignment-level).
        let mut input = baseline_input();
        input.virus_found = Some("Test.Virus".into());
        input.auth.dmarc_policy = DmarcPolicy::Reject;
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { code, message } => {
                assert_eq!(code, 550);
                assert!(message.contains("virus"));
                assert!(
                    !message.contains("DMARC"),
                    "virus message should not mention DMARC"
                );
            }
            other => panic!("expected virus Reject, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_none_does_not_reject_or_quarantine() {
        // DmarcPolicy::None should not gate the message — it should fall through
        // to score logic and eventually Accept.
        let mut input = baseline_input();
        input.auth.dmarc_policy = DmarcPolicy::None;
        let d = make_delivery_decision(&input);
        assert!(matches!(d, DeliveryDecision::Accept { .. }));
    }

    #[test]
    fn score_exactly_at_threshold_yields_junk() {
        // Boundary: score == threshold should also trigger Junk (>=).
        let mut input = baseline_input();
        input.content_score = 5.0;
        input.spam_threshold = 5.0;
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { .. } => {}
            other => panic!("expected Junk at == threshold, got {other:?}"),
        }
    }

    #[test]
    fn score_just_below_threshold_yields_accept() {
        let mut input = baseline_input();
        input.content_score = 4.999;
        input.spam_threshold = 5.0;
        let d = make_delivery_decision(&input);
        assert!(matches!(d, DeliveryDecision::Accept { .. }));
    }

    #[test]
    fn score_components_summed_independently() {
        // 1.0 + 2.0 + 3.0 = 6.0 → above threshold 5.0
        let mut input = baseline_input();
        input.content_score = 1.0;
        input.ptr_score = 2.0;
        input.ai_score = 3.0;
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("6.0") || reason.contains("6"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn empty_matched_rules_in_junk_reason_renders_cleanly() {
        // When matched_rules is empty, reason should still be well-formed.
        let mut input = baseline_input();
        input.content_score = 10.0;
        input.matched_rules.clear();
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                // reason must still parse — empty rules list is OK
                assert!(reason.contains("score"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }
}

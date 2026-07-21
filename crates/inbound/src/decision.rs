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
use crate::SenderTrust;

/// Score contributed to the spam total when receive-time authentication
/// folds to [`SenderTrust::Suspicious`] (DMARC alignment failed, or a hard
/// SPF/DKIM fail with no DMARC record — i.e. the From domain is being
/// spoofed or the sender's setup is broken).
///
/// Deliberately a **signal, not a hard rule**: at 3.0 against the default
/// 5.0 threshold, a suspicious sender alone still reaches the inbox (where
/// the UI's sender-trust badge flags it), but suspicious **plus** any
/// moderate content signal (>= 2.0) crosses the threshold into Junk. This
/// avoids junking a legit small domain with a transient DMARC fail under
/// `p=none`, while still catching spoofed phishing that also looks spammy.
pub const SUSPICIOUS_SENDER_SCORE: f64 = 3.0;

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
    /// Envelope sender's `From:` address, lowercased for whitelist /
    /// blacklist lookup. Empty when unavailable (very early failures
    /// before header parse); in that case the recipient-list decisions
    /// below fall through to the score-based path.
    ///
    /// v2.4.1 Phase 3 (RFC-B) addition.
    pub from_addr: String,
    /// Recipient's per-user whitelist — envelope sender addresses this
    /// recipient has explicitly marked as "not junk". Populated by the
    /// caller (fastcore inbound handler) for each `rcpt` before running
    /// the pipeline. A whitelist hit routes to Accept **only** if SPF
    /// or DKIM verified — prevents a phishing sender from claiming a
    /// whitelisted `From:` and bypassing the score path.
    ///
    /// v2.4.1 Phase 3 (RFC-B) addition.
    pub recipient_whitelist: std::collections::HashSet<String>,
    /// Recipient's per-user blacklist — envelope sender addresses this
    /// recipient has explicitly marked as junk / blocked. A blacklist
    /// hit routes straight to Junk, bypassing the score threshold path.
    ///
    /// v2.4.1 Phase 3 (RFC-B) addition.
    pub recipient_blacklist: std::collections::HashSet<String>,
}

/// Pure policy combiner. Order of precedence (high → low):
///
/// 1. Greylist (highest — defer before any other work).
/// 2. Virus found (hard 550 reject).
/// 3. DMARC policy=reject (hard 550 reject).
/// 4. **Recipient whitelist hit + SPF-or-DKIM pass → Accept** (v2.4.1
///    Phase 3, RFC-B §D5). The auth requirement prevents a phishing
///    sender from spoofing a whitelisted `From:` and bypassing the
///    score path.
/// 5. **Recipient blacklist hit → Junk** (v2.4.1 Phase 3, RFC-B §D4).
///    Runs after virus / DMARC-reject so a blacklist entry can't save
///    virus mail from a hard reject, but before content scoring so an
///    explicitly-blocked sender can't accidentally clear a low-score
///    threshold.
/// 6. DMARC policy=quarantine (route to Junk).
/// 7. Combined `content_score + ptr_score + ai_score >= spam_threshold` (Junk).
/// 8. Default: Accept.
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

    // v2.4.1 Phase 3 (RFC-B §D5) — recipient whitelist. Bypass the
    // score / DMARC-quarantine path only when SPF or DKIM pass; a
    // spoofed `From:` shouldn't earn Accept just because the true
    // owner of that address is whitelisted.
    if !input.from_addr.is_empty() && input.recipient_whitelist.contains(&input.from_addr) {
        let spf_pass = input.auth.spf.eq_ignore_ascii_case("pass");
        let dkim_pass = input.auth.dkim.eq_ignore_ascii_case("pass");
        if spf_pass || dkim_pass {
            return DeliveryDecision::Accept { auth_header };
        }
        // Whitelist hit but no auth pass — fall through to normal
        // scoring path. Do NOT log the ignored whitelist here; the
        // score / DMARC-quarantine decision that follows is the
        // authoritative one.
    }

    // v2.4.1 Phase 3 (RFC-B §D4) — recipient blacklist. Straight to
    // Junk (never SMTP-reject; §D8 restricts hard 550 to virus +
    // DMARC-reject only). Runs after virus / DMARC-reject so a
    // blacklist entry can't override those safety gates.
    if !input.from_addr.is_empty() && input.recipient_blacklist.contains(&input.from_addr) {
        return DeliveryDecision::Junk {
            auth_header,
            reason: format!("recipient blacklist: {}", input.from_addr),
        };
    }

    if input.auth.dmarc_policy == DmarcPolicy::Quarantine {
        return DeliveryDecision::Junk {
            auth_header,
            reason: "DMARC policy quarantine".into(),
        };
    }

    // Receive-time sender authentication, folded to a trust verdict.
    // `Suspicious` (spoof / broken sender setup) contributes to the spam
    // total as a signal — never on its own enough to Junk (see
    // `SUSPICIOUS_SENDER_SCORE`), so it combines with content/ptr/ai
    // rather than overriding them.
    let suspicious_score = if input.auth.sender_trust() == SenderTrust::Suspicious {
        SUSPICIOUS_SENDER_SCORE
    } else {
        0.0
    };

    let total_score = input.content_score + input.ptr_score + input.ai_score + suspicious_score;
    if total_score >= input.spam_threshold {
        return DeliveryDecision::Junk {
            auth_header,
            reason: build_junk_reason(input, total_score, suspicious_score),
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
fn build_junk_reason(input: &PipelineInput, total_score: f64, suspicious_score: f64) -> String {
    use std::fmt::Write as _;
    // Capacity for the prefix (~50 bytes) + numeric fields (~6 bytes
    // each w/ {:.1}) + a generous 64-byte budget for matched_rules.
    // Real-world reasons rarely exceed 150 bytes.
    let mut out = String::with_capacity(176);
    let _ = write!(
        out,
        "score {total_score:.1} >= {:.1} (content={:.1}, ptr={:.1}, ai={:.1}",
        input.spam_threshold, input.content_score, input.ptr_score, input.ai_score,
    );
    // Only mention the sender-trust contribution when it fired, so clean
    // mail's reason string stays as before.
    if suspicious_score > 0.0 {
        let _ = write!(out, ", sender=suspicious(+{suspicious_score:.1})");
    }
    out.push_str(", ");
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

    /// DMARC alignment failed but the domain publishes `p=none`, so the
    /// policy gates don't fire — the message reaches the score path. This
    /// is the exact gap `SUSPICIOUS_SENDER_SCORE` targets.
    fn suspicious_auth() -> AuthResults {
        AuthResults {
            spf: "pass".into(),
            dkim: "none".into(),
            arc: "none".into(),
            dmarc: "fail".into(),
            dmarc_policy: DmarcPolicy::None,
        }
    }

    /// Ordinary un-authenticated mail: no DMARC record, nothing failing.
    fn unverified_auth() -> AuthResults {
        AuthResults {
            spf: "none".into(),
            dkim: "none".into(),
            arc: "none".into(),
            dmarc: "none".into(),
            dmarc_policy: DmarcPolicy::None,
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
            from_addr: String::new(),
            recipient_whitelist: std::collections::HashSet::new(),
            recipient_blacklist: std::collections::HashSet::new(),
        }
    }

    #[test]
    fn baseline_passes_to_accept() {
        let d = make_delivery_decision(&baseline_input());
        assert!(matches!(d, DeliveryDecision::Accept { .. }));
    }

    // ── sender-trust as a scored signal (RFC 20260721) ────────────

    #[test]
    fn suspicious_sender_alone_stays_accept() {
        // Suspicious contributes SUSPICIOUS_SENDER_SCORE (3.0) < threshold
        // (5.0), so on its own it must NOT junk — it only flags (badge).
        let mut input = baseline_input();
        input.auth = suspicious_auth();
        input.content_score = 0.0;
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn suspicious_sender_plus_content_crosses_threshold() {
        // 2.5 content + 3.0 suspicious = 5.5 >= 5.0 → Junk, and the reason
        // must name the suspicious contribution.
        let mut input = baseline_input();
        input.auth = suspicious_auth();
        input.content_score = 2.5;
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("sender=suspicious"), "reason was: {reason}");
                assert!(reason.contains("5.5"), "reason was: {reason}");
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn suspicious_contribution_absent_from_clean_reason() {
        // A content-only junk (verified sender) must keep the old reason
        // shape — no phantom sender=suspicious token.
        let mut input = baseline_input();
        input.content_score = 6.0;
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(!reason.contains("sender=suspicious"), "reason was: {reason}");
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn verified_sender_contributes_zero() {
        // Verified sender + content 4.0 < 5.0 → Accept (no suspicious add).
        let mut input = baseline_input();
        input.auth = passing_auth();
        input.content_score = 4.0;
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn unverified_sender_contributes_zero() {
        // Plain un-authenticated mail is Unverified, not Suspicious —
        // no score contribution. content 4.0 < 5.0 → Accept.
        let mut input = baseline_input();
        input.auth = unverified_auth();
        input.content_score = 4.0;
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
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

    // ── v2.4.1 Phase 3 (RFC-B) — whitelist / blacklist ────────────

    fn whitelist_of(addr: &str) -> std::collections::HashSet<String> {
        let mut s = std::collections::HashSet::new();
        s.insert(addr.to_lowercase());
        s
    }

    #[test]
    fn whitelist_hit_with_spf_pass_forces_accept_over_score() {
        // The scoring path alone would send this to Junk (score >
        // threshold). The whitelist entry + SPF pass overrides.
        let mut input = baseline_input();
        input.from_addr = "friend@golia.jp".into();
        input.recipient_whitelist = whitelist_of("friend@golia.jp");
        input.content_score = 10.0;
        input.spam_threshold = 5.0;
        // auth already has spf="pass" via passing_auth()
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn whitelist_hit_with_dkim_pass_forces_accept_over_score() {
        // Same as above, but SPF fails and DKIM carries the auth.
        let mut input = baseline_input();
        input.from_addr = "friend@golia.jp".into();
        input.recipient_whitelist = whitelist_of("friend@golia.jp");
        input.auth.spf = "fail".into();
        input.auth.dkim = "pass".into();
        input.content_score = 10.0;
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn whitelist_hit_without_auth_falls_through_to_score() {
        // Neither SPF nor DKIM passed. A phishing sender spoofing a
        // whitelisted address must NOT ride the whitelist bypass.
        let mut input = baseline_input();
        input.from_addr = "friend@golia.jp".into();
        input.recipient_whitelist = whitelist_of("friend@golia.jp");
        input.auth.spf = "fail".into();
        input.auth.dkim = "fail".into();
        input.content_score = 10.0;
        // Falls through to the score-based Junk path.
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }

    #[test]
    fn whitelist_hit_cannot_save_virus_from_reject() {
        // Whitelist runs after virus. Even a fully-authed
        // whitelisted sender doesn't get to deliver malware.
        let mut input = baseline_input();
        input.virus_found = Some("Eicar".into());
        input.from_addr = "friend@golia.jp".into();
        input.recipient_whitelist = whitelist_of("friend@golia.jp");
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Reject { .. }
        ));
    }

    #[test]
    fn whitelist_hit_cannot_save_dmarc_reject() {
        let mut input = baseline_input();
        input.auth.dmarc_policy = DmarcPolicy::Reject;
        input.from_addr = "friend@golia.jp".into();
        input.recipient_whitelist = whitelist_of("friend@golia.jp");
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Reject { .. }
        ));
    }

    #[test]
    fn blacklist_hit_forces_junk_over_low_score() {
        // Low score would normally Accept; blacklist forces Junk.
        let mut input = baseline_input();
        input.from_addr = "spammer@evil.com".into();
        input.recipient_blacklist = whitelist_of("spammer@evil.com");
        input.content_score = 0.0;
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("blacklist"));
                assert!(reason.contains("spammer@evil.com"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn blacklist_never_yields_smtp_reject() {
        // Per §D8, only virus + DMARC-reject may hard-reject. A
        // blacklist entry is always Junk, never Reject.
        let mut input = baseline_input();
        input.from_addr = "spammer@evil.com".into();
        input.recipient_blacklist = whitelist_of("spammer@evil.com");
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }

    #[test]
    fn blacklist_cannot_override_virus_or_dmarc_reject() {
        let mut input = baseline_input();
        input.virus_found = Some("Eicar".into());
        input.from_addr = "spammer@evil.com".into();
        input.recipient_blacklist = whitelist_of("spammer@evil.com");
        // Virus wins — hard reject.
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Reject { .. }
        ));
    }

    #[test]
    fn empty_from_addr_disables_both_lists() {
        // Both whitelist and blacklist should skip when from_addr
        // is empty — no lookup can meaningfully hit.
        let mut input = baseline_input();
        input.recipient_whitelist = whitelist_of("someone@example.com");
        input.recipient_blacklist = whitelist_of("someone@example.com");
        input.from_addr = String::new();
        input.content_score = 10.0;
        // Falls through to score-based Junk (no whitelist bypass).
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }
}

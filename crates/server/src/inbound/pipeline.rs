use std::sync::Arc;

// Core types + RFC 8601 helpers come from the published mailrs-inbound crate.
// Re-exported here so existing in-crate callers can keep using
// `crate::inbound::pipeline::DeliveryDecision` etc.
pub use mailrs_inbound::{
    AuthResult, AuthResults, DeliveryDecision, DmarcPolicy, PipelineInput, build_auth_header,
    format_auth_results_header, make_delivery_decision,
};

use super::stages::mail_auth::MailAuthResolvers;
use super::stages::{
    AiScoringStage, ClamavStage, ContentScanStage, GreylistStage, MailAuthStage, PtrStage,
};
use mailrs_shield::greylist::{GreylistConfig, GreylistDb};

use hickory_resolver::TokioResolver;

use crate::dmarc_report::DmarcReportStore;
use crate::greylist_sync::GreylistListsHandle;

/// Build the inbound `mailrs_inbound::Pipeline` from the optional backends
/// configured at server startup. Each backend, when present, contributes its
/// corresponding `Stage` to the pipeline in fixed evaluation order:
/// `greylist → ptr → mail_auth → clamav → content_scan → ai_scoring`.
///
/// `content_scan` always runs (no external dependency); the others are
/// gated on `Some(_)` of the matching backend.
#[allow(clippy::too_many_arguments)]
pub fn build_inbound_pipeline(
    greylist_db: Option<Arc<GreylistDb>>,
    greylist_config: GreylistConfig,
    greylist_whitelist: GreylistListsHandle,
    resolver: Option<Arc<TokioResolver>>,
    mail_auth_resolvers: Option<MailAuthResolvers>,
    dmarc_report_store: Option<Arc<DmarcReportStore>>,
    clamav_addr: Option<String>,
    llm_provider: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    kevy: Option<crate::kevy_store::KevyStore>,
    spam_score_threshold: f64,
) -> mailrs_inbound::Pipeline {
    let mut builder = mailrs_inbound::Pipeline::builder().spam_threshold(spam_score_threshold);

    if let Some(db) = greylist_db {
        builder = builder.add(GreylistStage::new(db, greylist_config, greylist_whitelist));
    }
    if let Some(r) = resolver {
        builder = builder.add(PtrStage::new(r));
    }
    if let Some(resolvers) = mail_auth_resolvers {
        builder = builder.add(MailAuthStage::new(resolvers, dmarc_report_store));
    }
    if let Some(addr) = clamav_addr {
        builder = builder.add(ClamavStage::new(addr));
    }
    builder = builder.add(ContentScanStage::new());
    if let Some(provider) = llm_provider {
        builder = builder.add(AiScoringStage::new(provider, kevy, spam_score_threshold));
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_auth() -> AuthResults {
        AuthResults {
            spf: "pass".into(),
            dkim: "pass".into(),
            arc: "none".into(),
            dmarc: "pass".into(),
            dmarc_policy: DmarcPolicy::Pass,
        }
    }

    fn default_input() -> PipelineInput {
        PipelineInput {
            greylisted: false,
            auth: default_auth(),
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
    fn all_pass_accepts() {
        let d = make_delivery_decision(&default_input());
        assert!(matches!(d, DeliveryDecision::Accept { .. }));
    }

    #[test]
    fn dmarc_reject_returns_550() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::Reject,
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { code, message } => {
                assert_eq!(code, 550);
                assert!(message.contains("DMARC"));
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_quarantine_returns_junk() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::Quarantine,
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("DMARC policy quarantine"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_none_policy_accepts() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::None,
                ..default_auth()
            },
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn virus_detected_rejects() {
        let input = PipelineInput {
            virus_found: Some("Eicar".into()),
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { code, message } => {
                assert_eq!(code, 550);
                assert!(message.contains("virus detected (Eicar)"));
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[test]
    fn spam_score_above_threshold_is_junk() {
        let input = PipelineInput {
            content_score: 4.0,
            ptr_score: 1.5,
            spam_threshold: 5.0,
            matched_rules: vec!["missing_from".into()],
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }

    #[test]
    fn spam_score_at_threshold_is_junk() {
        let input = PipelineInput {
            content_score: 3.5,
            ptr_score: 1.5,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }

    #[test]
    fn spam_score_below_threshold_accepts() {
        let input = PipelineInput {
            content_score: 3.0,
            ptr_score: 1.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn greylisted_returns_greylist() {
        let input = PipelineInput {
            greylisted: true,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Greylist
        ));
    }

    #[test]
    fn greylist_has_highest_priority() {
        let input = PipelineInput {
            greylisted: true,
            virus_found: Some("Eicar".into()),
            content_score: 100.0,
            auth: AuthResults {
                dmarc_policy: DmarcPolicy::Reject,
                ..default_auth()
            },
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Greylist
        ));
    }

    #[test]
    fn virus_overrides_spam_score() {
        let input = PipelineInput {
            virus_found: Some("Trojan".into()),
            content_score: 1.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Reject { .. }
        ));
    }

    #[test]
    fn zero_score_zero_ptr_accepts() {
        let input = PipelineInput {
            content_score: 0.0,
            ptr_score: 0.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn junk_reason_contains_score_breakdown() {
        let input = PipelineInput {
            content_score: 4.0,
            ptr_score: 1.5,
            spam_threshold: 5.0,
            matched_rules: vec!["missing_from".into(), "html_only_no_text".into()],
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("5.5"));
                assert!(reason.contains("content=4.0"));
                assert!(reason.contains("ptr=1.5"));
                assert!(reason.contains("missing_from"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn auth_header_contains_all_four_methods() {
        let input = default_input();
        match make_delivery_decision(&input) {
            DeliveryDecision::Accept { auth_header } => {
                assert!(auth_header.contains("spf=pass"));
                assert!(auth_header.contains("dkim=pass"));
                assert!(auth_header.contains("arc=none"));
                assert!(auth_header.contains("dmarc=pass"));
            }
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    #[test]
    fn greylist_decision() {
        let input = PipelineInput {
            greylisted: true,
            ..default_input()
        };
        assert_eq!(make_delivery_decision(&input), DeliveryDecision::Greylist);
    }

    #[test]
    fn virus_rejects() {
        let input = PipelineInput {
            virus_found: Some("EICAR".into()),
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { code, message } => {
                assert_eq!(code, 550);
                assert!(message.contains("EICAR"));
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_reject_policy() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc_policy: DmarcPolicy::Reject,
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { code, .. } => assert_eq!(code, 550),
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_quarantine_junk() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc_policy: DmarcPolicy::Quarantine,
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("quarantine"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn combined_spam_score_triggers_junk() {
        let input = PipelineInput {
            content_score: 3.0,
            ptr_score: 1.5,
            ai_score: 1.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("5.5"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn low_spam_score_accepts() {
        let input = PipelineInput {
            content_score: 1.0,
            ptr_score: 0.5,
            ai_score: 0.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn virus_takes_priority_over_greylist() {
        // virus check happens after greylist, so greylist wins
        let input = PipelineInput {
            greylisted: true,
            virus_found: Some("test".into()),
            ..default_input()
        };
        assert_eq!(make_delivery_decision(&input), DeliveryDecision::Greylist);
    }

    #[test]
    fn hostname_in_auth_header() {
        let input = PipelineInput {
            hostname: "mail.golia.jp".into(),
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Accept { auth_header } => {
                assert!(auth_header.contains("mail.golia.jp"));
            }
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    // --- additional DMARC policy decision tests ---

    #[test]
    fn dmarc_reject_message_contains_policy_text() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::Reject,
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { message, .. } => {
                assert!(message.contains("5.7.1"));
                assert!(message.contains("DMARC"));
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_reject_overrides_high_spam_score() {
        // even if spam score is below threshold, DMARC reject should still reject
        let input = PipelineInput {
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::Reject,
                ..default_auth()
            },
            content_score: 0.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Reject { .. }
        ));
    }

    #[test]
    fn dmarc_quarantine_overrides_low_spam_score() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::Quarantine,
                ..default_auth()
            },
            content_score: 0.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }

    #[test]
    fn dmarc_none_with_high_spam_still_junks() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::None,
                ..default_auth()
            },
            content_score: 6.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }

    #[test]
    fn dmarc_pass_with_zero_score_accepts() {
        let input = PipelineInput {
            auth: AuthResults {
                spf: "pass".into(),
                dkim: "pass".into(),
                arc: "pass".into(),
                dmarc: "pass".into(),
                dmarc_policy: DmarcPolicy::Pass,
            },
            content_score: 0.0,
            ptr_score: 0.0,
            ai_score: 0.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn virus_overrides_dmarc_quarantine() {
        let input = PipelineInput {
            virus_found: Some("Trojan".into()),
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::Quarantine,
                ..default_auth()
            },
            ..default_input()
        };
        // virus reject should come before dmarc quarantine in priority
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { message, .. } => {
                assert!(message.contains("virus"));
            }
            other => panic!("expected Reject (virus), got {other:?}"),
        }
    }

    #[test]
    fn dmarc_reject_auth_header_contains_reason() {
        // this tests build_auth_header indirectly: dmarc_policy=Reject produces
        // reason="policy=reject" in the auth header — but it returns Reject before
        // we can see the header. Let's test with None policy instead.
        let input = PipelineInput {
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::None,
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Accept { auth_header } => {
                assert!(auth_header.contains("dmarc=fail"));
                assert!(auth_header.contains("reason=\"policy=none\""));
            }
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_pass_no_reason_in_auth_header() {
        let input = default_input();
        match make_delivery_decision(&input) {
            DeliveryDecision::Accept { auth_header } => {
                assert!(auth_header.contains("dmarc=pass"));
                // Pass should not have reason
                assert!(!auth_header.contains("reason="));
            }
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    #[test]
    fn dmarc_quarantine_auth_header_in_junk() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc: "fail".into(),
                dmarc_policy: DmarcPolicy::Quarantine,
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { auth_header, .. } => {
                assert!(auth_header.contains("dmarc=fail"));
                assert!(auth_header.contains("reason=\"policy=quarantine\""));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    // --- auth header content tests ---

    #[test]
    fn auth_header_spf_fail() {
        let input = PipelineInput {
            auth: AuthResults {
                spf: "fail".into(),
                dkim: "pass".into(),
                arc: "none".into(),
                dmarc: "pass".into(),
                dmarc_policy: DmarcPolicy::Pass,
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Accept { auth_header } => {
                assert!(auth_header.contains("spf=fail"));
            }
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    #[test]
    fn auth_header_arc_pass() {
        let input = PipelineInput {
            auth: AuthResults {
                arc: "pass".into(),
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Accept { auth_header } => {
                assert!(auth_header.contains("arc=pass"));
            }
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    #[test]
    fn auth_header_arc_fail() {
        let input = PipelineInput {
            auth: AuthResults {
                arc: "fail".into(),
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Accept { auth_header } => {
                assert!(auth_header.contains("arc=fail"));
            }
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    // --- spam scoring edge cases ---

    #[test]
    fn ai_score_contributes_to_junk() {
        let input = PipelineInput {
            content_score: 2.0,
            ptr_score: 1.0,
            ai_score: 2.5,
            spam_threshold: 5.0,
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("ai=2.5"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn just_below_threshold_accepts() {
        let input = PipelineInput {
            content_score: 2.0,
            ptr_score: 1.0,
            ai_score: 1.9, // total 4.9 < 5.0
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    #[test]
    fn exactly_at_threshold_is_junk() {
        let input = PipelineInput {
            content_score: 2.0,
            ptr_score: 1.0,
            ai_score: 2.0, // total 5.0 >= 5.0
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }

    #[test]
    fn zero_threshold_means_everything_is_junk() {
        let input = PipelineInput {
            content_score: 0.0,
            ptr_score: 0.0,
            ai_score: 0.0,
            spam_threshold: 0.0, // total 0.0 >= 0.0
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }

    #[test]
    fn negative_scores_possible() {
        // if somehow scores are negative (unlikely but possible)
        let input = PipelineInput {
            content_score: -1.0,
            ptr_score: 0.0,
            ai_score: 0.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ));
    }

    // --- DmarcPolicy enum tests ---

    #[test]
    fn dmarc_policy_debug_format() {
        assert_eq!(format!("{:?}", DmarcPolicy::Reject), "Reject");
        assert_eq!(format!("{:?}", DmarcPolicy::Quarantine), "Quarantine");
        assert_eq!(format!("{:?}", DmarcPolicy::None), "None");
        assert_eq!(format!("{:?}", DmarcPolicy::Pass), "Pass");
    }

    #[test]
    fn dmarc_policy_equality() {
        assert_eq!(DmarcPolicy::Reject, DmarcPolicy::Reject);
        assert_eq!(DmarcPolicy::Quarantine, DmarcPolicy::Quarantine);
        assert_eq!(DmarcPolicy::None, DmarcPolicy::None);
        assert_eq!(DmarcPolicy::Pass, DmarcPolicy::Pass);
        assert_ne!(DmarcPolicy::Reject, DmarcPolicy::Pass);
    }

    #[test]
    fn dmarc_policy_clone() {
        let p = DmarcPolicy::Quarantine;
        let c = p.clone();
        assert_eq!(p, c);
    }

    // --- DeliveryDecision enum tests ---

    #[test]
    fn delivery_decision_debug() {
        let d = DeliveryDecision::Greylist;
        assert_eq!(format!("{:?}", d), "Greylist");
    }

    #[test]
    fn delivery_decision_equality() {
        assert_eq!(DeliveryDecision::Greylist, DeliveryDecision::Greylist);
        assert_ne!(
            DeliveryDecision::Greylist,
            DeliveryDecision::Accept {
                auth_header: String::new()
            }
        );
    }

    // --- build_auth_header tests ---

    #[test]
    fn build_auth_header_all_results() {
        let header = build_auth_header("mx.test.com", "pass", "pass", "none", "pass", None);
        assert!(header.contains("Authentication-Results: mx.test.com"));
        assert!(header.contains("spf=pass"));
        assert!(header.contains("dkim=pass"));
        assert!(header.contains("arc=none"));
        assert!(header.contains("dmarc=pass"));
    }

    #[test]
    fn build_auth_header_with_dmarc_reason() {
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
    fn build_auth_header_no_dmarc_reason() {
        let header = build_auth_header("mx.test.com", "pass", "pass", "none", "pass", None);
        assert!(!header.contains("reason="));
    }

    #[test]
    fn build_auth_header_all_fail() {
        let header = build_auth_header(
            "mx.test.com",
            "fail",
            "fail",
            "fail",
            "fail",
            Some("policy=quarantine"),
        );
        assert!(header.contains("spf=fail"));
        assert!(header.contains("dkim=fail"));
        assert!(header.contains("arc=fail"));
        assert!(header.contains("dmarc=fail"));
        assert!(header.contains("reason=\"policy=quarantine\""));
    }

    #[test]
    fn build_auth_header_ends_with_crlf() {
        let header = build_auth_header("mx.test.com", "pass", "pass", "none", "pass", None);
        assert!(header.ends_with("\r\n"));
    }

    // --- priority ordering comprehensive tests ---

    #[test]
    fn priority_greylist_over_virus_over_dmarc_reject() {
        // greylist > virus > dmarc reject
        let input = PipelineInput {
            greylisted: true,
            virus_found: Some("Eicar".into()),
            auth: AuthResults {
                dmarc_policy: DmarcPolicy::Reject,
                ..default_auth()
            },
            content_score: 100.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert_eq!(make_delivery_decision(&input), DeliveryDecision::Greylist);
    }

    #[test]
    fn priority_virus_over_dmarc_reject() {
        let input = PipelineInput {
            virus_found: Some("Eicar".into()),
            auth: AuthResults {
                dmarc_policy: DmarcPolicy::Reject,
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { message, .. } => {
                assert!(
                    message.contains("virus"),
                    "should be virus reject, not DMARC"
                );
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }

    #[test]
    fn priority_dmarc_reject_over_spam() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc_policy: DmarcPolicy::Reject,
                ..default_auth()
            },
            content_score: 100.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { message, .. } => {
                assert!(message.contains("DMARC"));
            }
            other => panic!("expected DMARC Reject, got {other:?}"),
        }
    }

    #[test]
    fn priority_dmarc_quarantine_over_spam_accept() {
        // even with low spam score, quarantine still produces Junk
        let input = PipelineInput {
            auth: AuthResults {
                dmarc_policy: DmarcPolicy::Quarantine,
                ..default_auth()
            },
            content_score: 0.0,
            spam_threshold: 5.0,
            ..default_input()
        };
        assert!(matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Junk { .. }
        ));
    }

    // --- matched_rules in junk reason ---

    #[test]
    fn junk_reason_includes_all_matched_rules() {
        let input = PipelineInput {
            content_score: 6.0,
            spam_threshold: 5.0,
            matched_rules: vec!["rule_a".into(), "rule_b".into(), "rule_c".into()],
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("rule_a"));
                assert!(reason.contains("rule_b"));
                assert!(reason.contains("rule_c"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[test]
    fn junk_reason_empty_rules() {
        let input = PipelineInput {
            content_score: 6.0,
            spam_threshold: 5.0,
            matched_rules: vec![],
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Junk { reason, .. } => {
                assert!(reason.contains("6.0"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }
}

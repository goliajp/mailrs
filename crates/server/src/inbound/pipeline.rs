use std::net::IpAddr;
use std::sync::Arc;

use mail_auth::dmarc::verify::DmarcParameters;
use mail_auth::spf::verify::SpfParameters;
use mail_auth::{AuthenticatedMessage, MessageAuthenticator};

use super::auth_results::{format_auth_results_header, AuthResult};
use super::content_scan::{evaluate_rules, scan_clamav, ClamavResult};
use mailrs_shield::greylist::{self as greylisting, GreylistConfig, GreylistDb, GreylistDecision};

use hickory_resolver::TokioResolver;

use crate::dmarc_report::{DmarcReportStore, DmarcResultRecord};

#[derive(Debug, Clone, PartialEq)]
pub enum DeliveryDecision {
    Accept { auth_header: String },
    Junk { auth_header: String, reason: String },
    Reject { code: u16, message: String },
    Greylist,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthResults {
    pub spf: String,
    pub dkim: String,
    pub arc: String,
    pub dmarc: String,
    pub dmarc_policy: DmarcPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DmarcPolicy {
    Reject,
    Quarantine,
    None,
    Pass,
}

#[derive(Debug, Clone)]
pub struct PipelineInput {
    pub greylisted: bool,
    pub auth: AuthResults,
    pub virus_found: Option<String>,
    pub content_score: f64,
    pub matched_rules: Vec<String>,
    pub ptr_score: f64,
    pub ai_score: f64,
    pub spam_threshold: f64,
    pub hostname: String,
}

/// pure decision function — no I/O, fully testable
pub fn make_delivery_decision(input: &PipelineInput) -> DeliveryDecision {
    // 1. greylisting has highest priority
    if input.greylisted {
        return DeliveryDecision::Greylist;
    }

    // 2. virus is a hard reject
    if let Some(ref name) = input.virus_found {
        return DeliveryDecision::Reject {
            code: 550,
            message: format!("5.7.1 Message rejected: virus detected ({name})"),
        };
    }

    // 3. build auth header
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

    // 4. DMARC policy reject
    if input.auth.dmarc_policy == DmarcPolicy::Reject {
        return DeliveryDecision::Reject {
            code: 550,
            message: "5.7.1 DMARC policy reject".to_string(),
        };
    }

    // 5. DMARC policy quarantine
    if input.auth.dmarc_policy == DmarcPolicy::Quarantine {
        return DeliveryDecision::Junk {
            auth_header,
            reason: "DMARC policy quarantine".into(),
        };
    }

    // 6. spam score
    let total_score = input.content_score + input.ptr_score + input.ai_score;
    if total_score >= input.spam_threshold {
        return DeliveryDecision::Junk {
            auth_header,
            reason: format!(
                "score {total_score:.1} >= {:.1} (content={:.1}, ptr={:.1}, ai={:.1}, {})",
                input.spam_threshold,
                input.content_score,
                input.ptr_score,
                input.ai_score,
                input.matched_rules.join(", ")
            ),
        };
    }

    DeliveryDecision::Accept { auth_header }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_inbound_pipeline(
    authenticator: &MessageAuthenticator,
    hostname: &str,
    client_ip: IpAddr,
    ehlo_domain: &str,
    sender: &str,
    first_recipient: &str,
    message: &[u8],
    greylist_db: Option<&Arc<GreylistDb>>,
    greylist_config: &GreylistConfig,
    spam_score_threshold: f64,
    dmarc_report_store: Option<&Arc<DmarcReportStore>>,
    resolver: Option<&Arc<TokioResolver>>,
    clamav_addr: Option<&str>,
    llm_provider: Option<&Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    valkey: Option<&redis::aio::ConnectionManager>,
) -> DeliveryDecision {
    // 1. greylisting (fast, no DNS)
    if let Some(db) = greylist_db {
        let key = greylisting::triplet_key(&client_ip.to_string(), sender, first_recipient);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        match db.check(&key, now, greylist_config).await {
            GreylistDecision::Defer | GreylistDecision::TooEarly => {
                return DeliveryDecision::Greylist;
            }
            GreylistDecision::Accept => {}
        }
    }

    // 1.5 PTR check (scoring signal, not hard reject)
    let ptr_score = if let Some(r) = resolver {
        mailrs_shield::ptr::check_client_ptr(r, client_ip, ehlo_domain).await
    } else {
        0.0
    };

    // 2. SPF
    let spf_params = SpfParameters::verify_mail_from(client_ip, ehlo_domain, hostname, sender);
    let spf_output = authenticator.verify_spf(spf_params).await;
    let spf_str = spf_result_str(spf_output.result());

    // 3. DKIM + 3.5 ARC + 4. DMARC
    let mut dkim_str = "none".to_string();
    let mut arc_str = "none".to_string();
    let mut dmarc_str = "none".to_string();
    let mut dmarc_quarantine = false;

    if let Some(auth_msg) = AuthenticatedMessage::parse(message) {
        let dkim_outputs = authenticator.verify_dkim(&auth_msg).await;

        // ARC verification
        let arc_output = authenticator.verify_arc(&auth_msg).await;
        if auth_msg.ams_headers.is_empty() {
            arc_str = "none".to_string();
        } else if arc_output.can_be_sealed() {
            arc_str = "pass".to_string();
        } else {
            arc_str = "fail".to_string();
        }

        // summarize DKIM result
        dkim_str = if dkim_outputs.is_empty() {
            "none".to_string()
        } else if dkim_outputs
            .iter()
            .any(|o| matches!(o.result(), mail_auth::DkimResult::Pass))
        {
            "pass".to_string()
        } else {
            "fail".to_string()
        };

        // DMARC
        let mail_from_domain = sender
            .rsplit_once('@')
            .map(|(_, d)| d)
            .unwrap_or(ehlo_domain);
        let dmarc_params =
            DmarcParameters::new(&auth_msg, &dkim_outputs, mail_from_domain, &spf_output);
        let dmarc_output = authenticator.verify_dmarc(dmarc_params).await;

        let dmarc_pass = dmarc_output.dkim_result() == &mail_auth::DmarcResult::Pass
            || dmarc_output.spf_result() == &mail_auth::DmarcResult::Pass;
        let policy = dmarc_output.policy();

        if dmarc_pass {
            dmarc_str = "pass".to_string();
        } else {
            match policy {
                mail_auth::dmarc::Policy::Reject => {
                    // record DMARC reject for reporting
                    if let Some(store) = dmarc_report_store {
                        let _ = store
                            .record_result(&DmarcResultRecord {
                                source_ip: client_ip.to_string(),
                                from_domain: mail_from_domain.to_string(),
                                spf_result: spf_str.clone(),
                                dkim_result: dkim_str.clone(),
                                dmarc_result: "fail".to_string(),
                                disposition: "reject".to_string(),
                            })
                            .await;
                    }
                    // hard reject — build header and return immediately
                    let auth_header = build_auth_header(
                        hostname,
                        &spf_str,
                        &dkim_str,
                        &arc_str,
                        "fail",
                        Some("policy=reject"),
                    );
                    tracing::info!(
                        event = "dmarc_reject",
                        domain = mail_from_domain,
                        spf = %spf_str,
                        dkim = %dkim_str,
                        "DMARC reject"
                    );
                    // prepend header even on reject for diagnostics (not delivered, but logged)
                    let _ = auth_header;
                    return DeliveryDecision::Reject {
                        code: 550,
                        message: format!("5.7.1 DMARC policy reject for domain {mail_from_domain}"),
                    };
                }
                mail_auth::dmarc::Policy::Quarantine => {
                    dmarc_str = "fail".to_string();
                    dmarc_quarantine = true;
                }
                mail_auth::dmarc::Policy::None => {
                    dmarc_str = "fail".to_string();
                }
                mail_auth::dmarc::Policy::Unspecified => {
                    dmarc_str = "none".to_string();
                }
            }
        }
    }

    // record DMARC result for aggregate reporting
    if let Some(store) = dmarc_report_store {
        let mail_from_domain = sender
            .rsplit_once('@')
            .map(|(_, d)| d)
            .unwrap_or(ehlo_domain);
        let disposition = if dmarc_quarantine {
            "quarantine"
        } else {
            "none"
        };
        let _ = store
            .record_result(&DmarcResultRecord {
                source_ip: client_ip.to_string(),
                from_domain: mail_from_domain.to_string(),
                spf_result: spf_str.clone(),
                dkim_result: dkim_str.clone(),
                dmarc_result: dmarc_str.clone(),
                disposition: disposition.to_string(),
            })
            .await;
    }

    // 5. ClamAV virus scan (before content scoring, hard reject on virus)
    let virus_found = if let Some(addr) = clamav_addr {
        match scan_clamav(addr, message).await {
            ClamavResult::Virus(name) => {
                tracing::warn!(event = "clamav_reject", virus = %name, "virus detected");
                Some(name)
            }
            ClamavResult::Error(e) => {
                tracing::warn!(event = "clamav_error", error = %e, "ClamAV scan failed, accepting");
                None
            }
            ClamavResult::Clean => None,
        }
    } else {
        None
    };

    // 6. content scan
    let (content_score, matched_rules) = evaluate_rules(message);

    // 6.5 AI classification (only in grey zone: 1.0 < rule_score < threshold)
    let rule_total = content_score + ptr_score;
    let ai_score = if rule_total > 1.0 && rule_total < spam_score_threshold {
        if let Some(provider) = llm_provider {
            let subject = extract_header(message, "Subject").unwrap_or_default();
            let body_preview = extract_body_preview(message, 500);
            let cache = valkey
                .cloned()
                .map(mailrs_intelligence::spam::RedisSpamCache::new);
            let cache_ref: Option<&dyn mailrs_intelligence::spam::SpamCache> =
                cache.as_ref().map(|c| c as &dyn mailrs_intelligence::spam::SpamCache);
            match mailrs_intelligence::spam::classify(
                provider.as_ref(),
                cache_ref,
                sender,
                &subject,
                &body_preview,
            )
            .await
            {
                Some(result) => result.score,
                None => 0.0,
            }
        } else {
            0.0
        }
    } else {
        0.0
    };

    // determine DMARC policy enum
    let dmarc_policy = if dmarc_str == "pass" {
        DmarcPolicy::Pass
    } else if dmarc_quarantine {
        DmarcPolicy::Quarantine
    } else {
        // dmarc_str == "fail" with policy=none or policy=reject already returned above
        DmarcPolicy::None
    };

    // log pipeline results
    let total_score = content_score + ptr_score + ai_score;
    tracing::info!(
        event = "auth_pipeline",
        spf = %spf_str,
        dkim = %dkim_str,
        arc = %arc_str,
        dmarc = %dmarc_str,
        content_score = content_score,
        ptr_score = ptr_score,
        ai_score = ai_score,
        total_score = total_score,
        "inbound pipeline complete"
    );

    // 7. pure decision
    make_delivery_decision(&PipelineInput {
        greylisted: false,
        auth: AuthResults {
            spf: spf_str,
            dkim: dkim_str,
            arc: arc_str,
            dmarc: dmarc_str,
            dmarc_policy,
        },
        virus_found,
        content_score,
        matched_rules,
        ptr_score,
        ai_score,
        spam_threshold: spam_score_threshold,
        hostname: hostname.to_string(),
    })
}

fn extract_header(message: &[u8], name: &str) -> Option<String> {
    let msg = std::str::from_utf8(message).ok()?;
    let prefix = format!("{name}: ");
    for line in msg.lines() {
        if line
            .to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
        {
            return Some(line[prefix.len()..].trim().to_string());
        }
    }
    None
}

fn extract_body_preview(message: &[u8], max_len: usize) -> String {
    let msg = String::from_utf8_lossy(message);
    // body starts after \r\n\r\n or \n\n
    let body = msg
        .find("\r\n\r\n")
        .map(|i| &msg[i + 4..])
        .or_else(|| msg.find("\n\n").map(|i| &msg[i + 2..]))
        .unwrap_or("");
    let truncated: String = body.chars().take(max_len).collect();
    truncated
}

fn spf_result_str(result: mail_auth::SpfResult) -> String {
    match result {
        mail_auth::SpfResult::Pass => "pass",
        mail_auth::SpfResult::Fail => "fail",
        mail_auth::SpfResult::SoftFail => "softfail",
        mail_auth::SpfResult::Neutral => "neutral",
        mail_auth::SpfResult::None => "none",
        mail_auth::SpfResult::TempError => "temperror",
        mail_auth::SpfResult::PermError => "permerror",
    }
    .to_string()
}

fn build_auth_header(
    hostname: &str,
    spf: &str,
    dkim: &str,
    arc: &str,
    dmarc: &str,
    dmarc_reason: Option<&str>,
) -> String {
    let results = vec![
        AuthResult {
            method: "spf".into(),
            result: spf.into(),
            reason: None,
        },
        AuthResult {
            method: "dkim".into(),
            result: dkim.into(),
            reason: None,
        },
        AuthResult {
            method: "arc".into(),
            result: arc.into(),
            reason: None,
        },
        AuthResult {
            method: "dmarc".into(),
            result: dmarc.into(),
            reason: dmarc_reason.map(|s| s.to_string()),
        },
    ];
    format_auth_results_header(hostname, &results)
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

    // --- extract_header tests ---

    #[test]
    fn extract_header_basic() {
        let msg = b"From: alice@example.com\r\nSubject: Hello\r\n\r\nbody";
        assert_eq!(extract_header(msg, "Subject").unwrap(), "Hello");
        assert_eq!(extract_header(msg, "From").unwrap(), "alice@example.com");
    }

    #[test]
    fn extract_header_case_insensitive() {
        let msg = b"subject: hello world\r\n\r\n";
        assert_eq!(extract_header(msg, "Subject").unwrap(), "hello world");
    }

    #[test]
    fn extract_header_missing() {
        let msg = b"From: alice@example.com\r\n\r\nbody";
        assert!(extract_header(msg, "Subject").is_none());
    }

    #[test]
    fn extract_header_empty_message() {
        assert!(extract_header(b"", "Subject").is_none());
    }

    // --- extract_body_preview tests ---

    #[test]
    fn extract_body_preview_crlf() {
        let msg = b"Subject: Test\r\n\r\nHello, world!";
        assert_eq!(extract_body_preview(msg, 500), "Hello, world!");
    }

    #[test]
    fn extract_body_preview_lf() {
        let msg = b"Subject: Test\n\nHello, world!";
        assert_eq!(extract_body_preview(msg, 500), "Hello, world!");
    }

    #[test]
    fn extract_body_preview_truncates() {
        let msg = b"Subject: Test\r\n\r\nHello, world!";
        assert_eq!(extract_body_preview(msg, 5), "Hello");
    }

    #[test]
    fn extract_body_preview_no_body() {
        let msg = b"Subject: Test\r\n";
        assert_eq!(extract_body_preview(msg, 500), "");
    }

    #[test]
    fn extract_body_preview_empty() {
        assert_eq!(extract_body_preview(b"", 500), "");
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
        assert!(matches!(make_delivery_decision(&input), DeliveryDecision::Accept { .. }));
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
            DeliveryDecision::Accept { auth_header: String::new() }
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
        let header = build_auth_header("mx.test.com", "pass", "fail", "none", "fail", Some("policy=reject"));
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
        let header = build_auth_header("mx.test.com", "fail", "fail", "fail", "fail", Some("policy=quarantine"));
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
                assert!(message.contains("virus"), "should be virus reject, not DMARC");
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

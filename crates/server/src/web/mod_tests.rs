//! Tests for `mod` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

// --- validate_domains ---

fn make_perms(domains: &[&str]) -> crate::permission::EffectivePermissions {
    use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo};
    let groups: Vec<AccountGroup> = domains
        .iter()
        .map(|d| AccountGroup {
            group: GroupInfo {
                id: 1,
                name: "user".into(),
                domain: Some(d.to_string()),
                description: String::new(),
                is_builtin: false,
                created_at: 0,
            },
            permissions: vec!["mail.send".into(), "mail.read".into()],
        })
        .collect();
    compute_effective_permissions(&groups, &[], &[])
}

fn make_empty_perms() -> crate::permission::EffectivePermissions {
    crate::permission::compute_effective_permissions(&[], &[], &[])
}

#[test]
fn validate_domains_returns_none_when_param_is_none() {
    assert!(validate_domains(None, &make_empty_perms()).is_none());
}

#[test]
fn validate_domains_returns_none_when_param_is_empty() {
    assert!(validate_domains(Some(""), &make_empty_perms()).is_none());
}

#[test]
fn validate_domains_returns_none_when_no_accessible_domains() {
    assert!(validate_domains(Some("example.com"), &make_empty_perms()).is_none());
}

#[test]
fn validate_domains_returns_allowed_domain() {
    let perms = make_perms(&["example.com"]);
    let result = validate_domains(Some("example.com"), &perms);
    assert_eq!(result, Some(vec!["example.com".to_string()]));
}

#[test]
fn validate_domains_filters_unauthorized_domains() {
    let perms = make_perms(&["example.com"]);
    let result = validate_domains(Some("example.com,evil.com"), &perms);
    assert_eq!(result, Some(vec!["example.com".to_string()]));
}

#[test]
fn validate_domains_returns_none_when_all_domains_unauthorized() {
    let perms = make_perms(&["example.com"]);
    let result = validate_domains(Some("evil.com"), &perms);
    assert!(result.is_none());
}

#[test]
fn validate_domains_handles_multiple_allowed_domains() {
    let perms = make_perms(&["example.com", "example.org"]);
    let result = validate_domains(Some("example.com,example.org"), &perms);
    assert_eq!(
        result,
        Some(vec!["example.com".to_string(), "example.org".to_string()])
    );
}

#[test]
fn validate_domains_trims_whitespace() {
    let perms = make_perms(&["example.com"]);
    let result = validate_domains(Some("  example.com  ,  "), &perms);
    assert_eq!(result, Some(vec!["example.com".to_string()]));
}

#[test]
fn validate_domains_skips_empty_segments() {
    let perms = make_perms(&["example.com"]);
    let result = validate_domains(Some(",example.com,,"), &perms);
    assert_eq!(result, Some(vec!["example.com".to_string()]));
}

// --- classify_email ---

#[test]
fn classify_safe_domain_noreply() {
    let (cat, score) = classify_email(
        "noreply@github.com",
        "You have a new notification",
        Some("Someone mentioned you in a PR"),
        None,
    );
    // noreply@ prefix triggers notification category even for safe domains
    assert_eq!(cat, "notification");
    assert_eq!(score, 0, "safe domain should have zero risk");
}

#[test]
fn classify_notification_sender() {
    let (cat, score) = classify_email(
        "notifications@facebookmail.com",
        "You have a new friend request",
        Some("John wants to connect"),
        None,
    );
    assert_eq!(cat, "notification");
    assert!(score <= 15);
}

#[test]
fn classify_promotion_with_unsubscribe() {
    let (cat, _score) = classify_email(
        "offers@shop.example.com",
        "Big Summer Sale!",
        Some("Check our latest deals. Click to unsubscribe"),
        None,
    );
    assert_eq!(cat, "promotion");
}

#[test]
fn classify_promotion_with_marketing_keywords() {
    let (cat, _score) = classify_email(
        "news@store.example.com",
        "Newsletter: Special Discount",
        Some("Check our latest newsletter. Click to unsubscribe."),
        None,
    );
    assert_eq!(cat, "promotion");
}

#[test]
fn classify_spam_multiple_signals() {
    let (cat, score) = classify_email(
        "unknown@sketchy.example.com",
        "URGENT: You are a winner!",
        Some("Click here to claim your prize. Act now, limited time!"),
        None,
    );
    assert!(
        cat == "spam" || cat == "scam",
        "expected spam or scam, got {cat}"
    );
    assert!(score >= 40, "spam score should be >= 40, got {score}");
}

#[test]
fn classify_scam_phishing_signals() {
    let (cat, score) = classify_email(
        "security@phisher.example.com",
        "Your account has been suspended",
        Some("Login immediately to verify your account. Confirm your identity. Your password needs updating."),
        None,
    );
    assert_eq!(cat, "scam");
    assert!(score >= 60, "phishing score should be >= 60, got {score}");
}

#[test]
fn classify_detects_tracking_pixels() {
    let (cat, _score) = classify_email(
        "info@tracker.example.com",
        "Weekly Update",
        Some("Here is your update"),
        Some("<html><body><img src='https://t.example.com/px' width=\"1\" height=\"1\" /></body></html>"),
    );
    assert_eq!(cat, "promotion");
}

#[test]
fn classify_detects_many_links() {
    let links = "<a href='#'>link</a>".repeat(25);
    let html = format!("<html><body>{links}</body></html>");
    let (cat, score) = classify_email(
        "info@newsletter.example.com",
        "Links roundup",
        None,
        Some(&html),
    );
    assert!(score >= 5, "many links should add to score, got {score}");
    assert!(cat == "promotion" || cat == "general");
}

#[test]
fn classify_plain_personal_email() {
    let (cat, score) = classify_email(
        "friend@personal.example.com",
        "Dinner tonight?",
        Some("Hey, want to grab dinner at 7pm?"),
        None,
    );
    assert_eq!(cat, "personal");
    assert_eq!(score, 0);
}

#[test]
fn classify_general_email_with_low_score() {
    let (cat, score) = classify_email(
        "support@company.example.com",
        "Your ticket has been updated",
        Some("We have an update on your support ticket #12345"),
        None,
    );
    assert!(cat == "personal" || cat == "general");
    assert!(score < 40);
}

#[test]
fn classify_safe_domain_resists_spam_signals() {
    let (cat, score) = classify_email(
        "noreply@google.com",
        "Urgent: verify your account",
        Some("Please verify your account"),
        None,
    );
    assert!(score < 60, "safe domain should dampen score, got {score}");
    assert_ne!(cat, "scam");
}

#[test]
fn classify_japanese_spam_signals() {
    let (cat, score) = classify_email(
        "unknown@example.com",
        "至急ご確認ください",
        Some("当選おめでとうございます。緊急のお知らせです。"),
        None,
    );
    assert!(score >= 40, "japanese spam signals should raise score, got {score}");
    assert!(cat == "spam" || cat == "scam");
}

#[test]
fn classify_chinese_phishing_signals() {
    let (cat, score) = classify_email(
        "security@example.com",
        "账户异常通知",
        Some("您的账号被锁定，请立即修改密码"),
        None,
    );
    assert!(score >= 40, "chinese phish signals should raise score, got {score}");
    assert!(cat == "spam" || cat == "scam");
}

#[test]
fn classify_notification_with_noreply_prefix() {
    let (cat, score) = classify_email(
        "noreply@some-service.example.com",
        "Your order has shipped",
        Some("Your package is on the way"),
        None,
    );
    assert_eq!(cat, "notification");
    assert!(score <= 15);
}

#[test]
fn classify_score_clamped_to_100() {
    let (_, score) = classify_email(
        "scammer@evil.example.com",
        "URGENT: winner! congratulations! lottery prize!",
        Some("click here, act now, limited time, verify your account, suspended, locked, password, login immediately, confirm your identity, アカウントが制限, アカウントを確認, 账户异常, 账号被锁, 密码, パスワード, 当選, 至急, 緊急, 中奖, 恭喜, 紧急"),
        None,
    );
    assert!(score <= 100, "score should be clamped to 100, got {score}");
}

#[test]
fn classify_score_never_negative() {
    let (_, score) = classify_email(
        "noreply@github.com",
        "PR review requested",
        Some("Please review this pull request"),
        None,
    );
    assert_eq!(score, 0);
}

#[test]
fn classify_html_unsubscribe_in_html_only() {
    let (cat, _score) = classify_email(
        "news@example.com",
        "Monthly Report",
        None,
        Some("<html><body><p>Report content</p><a href='#'>unsubscribe</a></body></html>"),
    );
    assert_eq!(cat, "promotion");
}

#[test]
fn classify_case_insensitive() {
    let (cat1, score1) = classify_email(
        "NOREPLY@GITHUB.COM",
        "PR Review",
        Some("Please review"),
        None,
    );
    let (cat2, score2) = classify_email(
        "noreply@github.com",
        "PR Review",
        Some("Please review"),
        None,
    );
    assert_eq!(cat1, cat2);
    assert_eq!(score1, score2);
}

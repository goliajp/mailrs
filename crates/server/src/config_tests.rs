//! Tests for [`super`](config).
//!
//! Extracted from the production file per the one-file-one-thing
//! policy. Re-attached as a sibling module via `#[path = "config_tests.rs"] mod tests;`
//! inside `config.rs` so `super::*` still resolves to the prod items.

use super::*;
use std::sync::Mutex;

// env vars are process-global, so from_env tests must run serially
static ENV_LOCK: Mutex<()> = Mutex::new(());

// helper: clear all MAILRS_* env vars to isolate each from_env test
fn clear_mailrs_env() {
    let keys: Vec<String> = std::env::vars()
        .filter(|(k, _)| k.starts_with("MAILRS_"))
        .map(|(k, _)| k)
        .collect();
    for k in keys {
        unsafe { std::env::remove_var(&k) };
    }
}

// =====================================================================
// tls_mode
// =====================================================================

#[test]
fn tls_mode_none_when_no_cert_no_acme() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.tls_mode(), TlsMode::None);
}

#[test]
fn tls_mode_manual_when_cert_and_key_set() {
    let cfg = ServerConfig {
        tls_cert: Some(PathBuf::from("/etc/ssl/cert.pem")),
        tls_key: Some(PathBuf::from("/etc/ssl/key.pem")),
        ..ServerConfig::default()
    };
    assert_eq!(cfg.tls_mode(), TlsMode::Manual);
}

#[test]
fn tls_mode_none_when_only_cert_without_key() {
    let cfg = ServerConfig {
        tls_cert: Some(PathBuf::from("/etc/ssl/cert.pem")),
        tls_key: None,
        ..ServerConfig::default()
    };
    assert_eq!(cfg.tls_mode(), TlsMode::None);
}

#[test]
fn tls_mode_none_when_only_key_without_cert() {
    let cfg = ServerConfig {
        tls_cert: None,
        tls_key: Some(PathBuf::from("/etc/ssl/key.pem")),
        ..ServerConfig::default()
    };
    assert_eq!(cfg.tls_mode(), TlsMode::None);
}

#[test]
fn tls_mode_acme_takes_precedence_over_manual() {
    let cfg = ServerConfig {
        tls_cert: Some(PathBuf::from("/etc/ssl/cert.pem")),
        tls_key: Some(PathBuf::from("/etc/ssl/key.pem")),
        acme_email: Some("admin@example.com".into()),
        ..ServerConfig::default()
    };
    assert_eq!(cfg.tls_mode(), TlsMode::Acme);
}

#[test]
fn tls_mode_acme_without_manual_certs() {
    let cfg = ServerConfig {
        acme_email: Some("admin@example.com".into()),
        ..ServerConfig::default()
    };
    assert_eq!(cfg.tls_mode(), TlsMode::Acme);
}

// =====================================================================
// has_tls
// =====================================================================

#[test]
fn has_tls_false_by_default() {
    let cfg = ServerConfig::default();
    assert!(!cfg.has_tls());
}

#[test]
fn has_tls_true_when_both_cert_and_key() {
    let cfg = ServerConfig {
        tls_cert: Some(PathBuf::from("/cert.pem")),
        tls_key: Some(PathBuf::from("/key.pem")),
        ..ServerConfig::default()
    };
    assert!(cfg.has_tls());
}

#[test]
fn has_tls_false_when_only_cert() {
    let cfg = ServerConfig {
        tls_cert: Some(PathBuf::from("/cert.pem")),
        ..ServerConfig::default()
    };
    assert!(!cfg.has_tls());
}

#[test]
fn has_tls_false_when_only_key() {
    let cfg = ServerConfig {
        tls_key: Some(PathBuf::from("/key.pem")),
        ..ServerConfig::default()
    };
    assert!(!cfg.has_tls());
}

// =====================================================================
// validate — hostname warnings
// =====================================================================

#[test]
fn validate_warns_on_default_hostname() {
    let cfg = ServerConfig {
        hostname: "localhost".into(),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(warnings.iter().any(|w| w.contains("MAILRS_HOSTNAME")));
}

#[test]
fn validate_warns_on_empty_hostname() {
    let cfg = ServerConfig {
        hostname: String::new(),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(warnings.iter().any(|w| w.contains("MAILRS_HOSTNAME")));
}

#[test]
fn validate_no_hostname_warning_for_fqdn() {
    let cfg = ServerConfig {
        hostname: "mail.example.org".into(),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("MAILRS_HOSTNAME")));
}

// =====================================================================
// validate — local domains warnings
// =====================================================================

#[test]
fn validate_warns_on_empty_local_domains() {
    let cfg = ServerConfig::default();
    let warnings = cfg.validate();
    assert!(warnings.iter().any(|w| w.contains("MAILRS_LOCAL_DOMAINS")));
}

#[test]
fn validate_no_domain_warning_when_domains_set() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("MAILRS_LOCAL_DOMAINS")));
    assert!(!warnings.iter().any(|w| w.contains("MAILRS_HOSTNAME")));
}

#[test]
fn validate_no_domain_warning_with_multiple_domains() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into(), "example.org".into()],
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("MAILRS_LOCAL_DOMAINS")));
}

// =====================================================================
// validate — TLS warnings
// =====================================================================

#[test]
fn validate_warns_on_no_tls() {
    let cfg = ServerConfig::default();
    let warnings = cfg.validate();
    assert!(warnings.iter().any(|w| w.contains("No TLS configured")));
}

#[test]
fn validate_no_tls_warning_when_acme() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("No TLS configured")));
}

#[test]
fn validate_no_tls_warning_when_manual_certs() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        tls_cert: Some(PathBuf::from("/cert.pem")),
        tls_key: Some(PathBuf::from("/key.pem")),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("No TLS configured")));
}

// =====================================================================
// validate — DKIM warnings
// =====================================================================

#[test]
fn validate_warns_dkim_selector_without_key() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        dkim_selector: Some("default".into()),
        dkim_private_key_path: None,
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(warnings.iter().any(|w| w.contains("DKIM")));
}

#[test]
fn validate_no_dkim_warning_when_both_set() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        dkim_selector: Some("default".into()),
        dkim_private_key_path: Some(PathBuf::from("/etc/dkim/key.pem")),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("DKIM")));
}

#[test]
fn validate_no_dkim_warning_when_neither_set() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        dkim_selector: None,
        dkim_private_key_path: None,
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("DKIM")));
}

#[test]
fn validate_no_dkim_warning_when_only_key_set() {
    // key path without selector should not warn (selector check drives the warning)
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        dkim_selector: None,
        dkim_private_key_path: Some(PathBuf::from("/key.pem")),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("DKIM")));
}

// =====================================================================
// validate — MTA-STS warnings
// =====================================================================

#[test]
fn validate_warns_mta_sts_mode_without_mx() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        mta_sts_mode: Some("enforce".into()),
        mta_sts_mx: vec![],
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(warnings.iter().any(|w| w.contains("MTA-STS")));
}

#[test]
fn validate_no_mta_sts_warning_when_mx_set() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        mta_sts_mode: Some("enforce".into()),
        mta_sts_mx: vec!["mx.example.com".into()],
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("MTA-STS")));
}

#[test]
fn validate_no_mta_sts_warning_when_mode_not_set() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        mta_sts_mode: None,
        mta_sts_mx: vec![],
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("MTA-STS")));
}

#[test]
fn validate_mta_sts_testing_mode_without_mx_warns() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        mta_sts_mode: Some("testing".into()),
        mta_sts_mx: vec![],
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(warnings.iter().any(|w| w.contains("MTA-STS")));
}

// =====================================================================
// validate — valkey url
// =====================================================================

#[test]
fn validate_warns_on_invalid_valkey_url() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        valkey_url: Some("not-a-valid-url".into()),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(warnings.iter().any(|w| w.contains("MAILRS_VALKEY_URL")));
}

#[test]
fn validate_no_valkey_warning_for_valid_url() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        valkey_url: Some("redis://localhost:6379".into()),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("MAILRS_VALKEY_URL")));
}

#[test]
fn validate_no_valkey_warning_when_none() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        valkey_url: None,
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(!warnings.iter().any(|w| w.contains("MAILRS_VALKEY_URL")));
}

// =====================================================================
// validate — warning count / combinations
// =====================================================================

#[test]
fn validate_clean_config_has_no_warnings() {
    let cfg = ServerConfig {
        hostname: "mx.example.com".into(),
        local_domains: vec!["example.com".into()],
        acme_email: Some("admin@example.com".into()),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
}

#[test]
fn validate_default_config_has_exactly_three_warnings() {
    // default: hostname is mx.mailrs.local (not localhost/empty, no warning),
    // empty local_domains, no TLS => 2 warnings
    let cfg = ServerConfig::default();
    let warnings = cfg.validate();
    // hostname "mx.mailrs.local" is valid (not localhost/empty)
    assert!(warnings.iter().any(|w| w.contains("MAILRS_LOCAL_DOMAINS")));
    assert!(warnings.iter().any(|w| w.contains("No TLS configured")));
    assert_eq!(warnings.len(), 2);
}

#[test]
fn validate_multiple_issues_all_reported() {
    let cfg = ServerConfig {
        hostname: "localhost".into(),
        local_domains: vec![],
        dkim_selector: Some("sel".into()),
        dkim_private_key_path: None,
        mta_sts_mode: Some("enforce".into()),
        mta_sts_mx: vec![],
        valkey_url: Some("garbage".into()),
        ..ServerConfig::default()
    };
    let warnings = cfg.validate();
    assert!(warnings.iter().any(|w| w.contains("MAILRS_HOSTNAME")));
    assert!(warnings.iter().any(|w| w.contains("MAILRS_LOCAL_DOMAINS")));
    assert!(warnings.iter().any(|w| w.contains("No TLS configured")));
    assert!(warnings.iter().any(|w| w.contains("DKIM")));
    assert!(warnings.iter().any(|w| w.contains("MTA-STS")));
    assert!(warnings.iter().any(|w| w.contains("MAILRS_VALKEY_URL")));
    assert_eq!(warnings.len(), 6);
}

// =====================================================================
// default values — comprehensive
// =====================================================================

#[test]
fn default_config_has_expected_ports() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.smtp_port, 2525);
    assert_eq!(cfg.submission_port, 2587);
    assert_eq!(cfg.smtps_port, 2465);
    assert_eq!(cfg.imap_port, 1143);
    assert_eq!(cfg.imaps_port, 1993);
    assert_eq!(cfg.web_port, 3100);
}

#[test]
fn default_hostname() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.hostname, "mx.mailrs.local");
}

#[test]
fn default_maildir_root() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.maildir_root, "/tmp/mailrs");
}

#[test]
fn default_smuggle_protection_is_permissive() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.smuggle_protection, SmuggleProtection::Permissive);
}

#[test]
fn default_ai_is_disabled() {
    let cfg = ServerConfig::default();
    assert!(!cfg.ai_analysis_enabled);
    assert_eq!(cfg.llm_url, "https://devops.golia.jp/api/llm/complete");
}

#[test]
fn default_rate_limit_values() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.rate_limit_capacity, 10);
    assert!((cfg.rate_limit_refill - 1.0).abs() < f64::EPSILON);
}

#[test]
fn default_greylist_delay() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.greylist_delay_secs, 300);
}

#[test]
fn default_dnsbl_and_antispam_enabled() {
    let cfg = ServerConfig::default();
    assert!(cfg.dnsbl_enabled);
    assert!(cfg.antispam_enabled);
}

#[test]
fn default_spam_score_threshold() {
    let cfg = ServerConfig::default();
    assert!((cfg.spam_score_threshold - 5.0).abs() < f64::EPSILON);
}

#[test]
fn default_acme_dir() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.acme_dir, PathBuf::from("/data/acme"));
}

#[test]
fn default_acme_staging_false() {
    let cfg = ServerConfig::default();
    assert!(!cfg.acme_staging);
}

#[test]
fn default_mta_sts_max_age() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.mta_sts_max_age, 604800); // 7 days
}

#[test]
fn default_mta_sts_id_is_timestamp_format() {
    let cfg = ServerConfig::default();
    // format is YYYYMMDDHHmmSS — 14 digits
    assert_eq!(cfg.mta_sts_id.len(), 14);
    assert!(cfg.mta_sts_id.chars().all(|c| c.is_ascii_digit()));
}

#[test]
fn default_optional_fields_are_none() {
    let cfg = ServerConfig::default();
    assert!(cfg.tls_cert.is_none());
    assert!(cfg.tls_key.is_none());
    assert!(cfg.users_file.is_none());
    assert!(cfg.dkim_selector.is_none());
    assert!(cfg.dkim_domain.is_none());
    assert!(cfg.dkim_private_key_path.is_none());
    assert!(cfg.web_static_dir.is_none());
    assert!(cfg.acme_email.is_none());
    assert!(cfg.mta_sts_mode.is_none());
    assert!(cfg.clamav_addr.is_none());
    assert!(cfg.pg_url.is_none());
    assert!(cfg.valkey_url.is_none());
}

#[test]
fn default_empty_lists() {
    let cfg = ServerConfig::default();
    assert!(cfg.local_domains.is_empty());
    assert!(cfg.dnsbl_zones.is_empty());
    assert!(cfg.acme_domains.is_empty());
    assert!(cfg.mta_sts_mx.is_empty());
}

#[test]
fn default_auth_guard_values() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.auth_max_failures_account, 5);
    assert_eq!(cfg.auth_account_window_secs, 900);
    assert_eq!(cfg.auth_base_lockout_secs, 1800);
    assert_eq!(cfg.auth_max_failures_ip, 20);
    assert_eq!(cfg.auth_ip_window_secs, 3600);
    assert_eq!(cfg.auth_ip_base_lockout_secs, 3600);
    assert!((cfg.auth_backoff_multiplier - 2.0).abs() < f64::EPSILON);
    assert_eq!(cfg.auth_max_lockout_secs, 86400);
}

// =====================================================================
// from_env — string fields
// =====================================================================

#[test]
fn from_env_hostname() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_HOSTNAME", "mail.test.io") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.hostname, "mail.test.io");
    clear_mailrs_env();
}

#[test]
fn from_env_maildir() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_MAILDIR", "/var/mail") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.maildir_root, "/var/mail");
    clear_mailrs_env();
}

// =====================================================================
// from_env — port parsing
// =====================================================================

#[test]
fn from_env_smtp_port() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_PORT", "25") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smtp_port, 25);
    clear_mailrs_env();
}

#[test]
fn from_env_submission_port() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_SUBMISSION_PORT", "587") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.submission_port, 587);
    clear_mailrs_env();
}

#[test]
fn from_env_smtps_port() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_SMTPS_PORT", "465") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smtps_port, 465);
    clear_mailrs_env();
}

#[test]
fn from_env_web_port() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_WEB_PORT", "8080") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.web_port, 8080);
    clear_mailrs_env();
}

#[test]
fn from_env_imap_port() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_IMAP_PORT", "143") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.imap_port, 143);
    clear_mailrs_env();
}

#[test]
fn from_env_imaps_port() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_IMAPS_PORT", "993") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.imaps_port, 993);
    clear_mailrs_env();
}

#[test]
fn from_env_invalid_port_keeps_default() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_PORT", "not_a_number") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smtp_port, 2525); // default preserved
    clear_mailrs_env();
}

#[test]
fn from_env_port_overflow_keeps_default() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_PORT", "99999") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smtp_port, 2525); // u16 overflow, parse fails
    clear_mailrs_env();
}

#[test]
fn from_env_port_zero_accepted() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_PORT", "0") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smtp_port, 0);
    clear_mailrs_env();
}

#[test]
fn from_env_port_max_u16() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_PORT", "65535") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smtp_port, 65535);
    clear_mailrs_env();
}

#[test]
fn from_env_negative_port_keeps_default() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_PORT", "-1") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smtp_port, 2525);
    clear_mailrs_env();
}

// =====================================================================
// from_env — TLS cert/key paths
// =====================================================================

#[test]
fn from_env_tls_cert_and_key() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_TLS_CERT", "/etc/ssl/cert.pem") };
    unsafe { std::env::set_var("MAILRS_TLS_KEY", "/etc/ssl/key.pem") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.tls_cert, Some(PathBuf::from("/etc/ssl/cert.pem")));
    assert_eq!(cfg.tls_key, Some(PathBuf::from("/etc/ssl/key.pem")));
    clear_mailrs_env();
}

#[test]
fn from_env_users_file() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_USERS_FILE", "/etc/mailrs/users.toml") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.users_file, Some(PathBuf::from("/etc/mailrs/users.toml")));
    clear_mailrs_env();
}

// =====================================================================
// from_env — comma-separated lists
// =====================================================================

#[test]
fn from_env_local_domains_single() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_LOCAL_DOMAINS", "example.com") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.local_domains, vec!["example.com"]);
    clear_mailrs_env();
}

#[test]
fn from_env_local_domains_multiple() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_LOCAL_DOMAINS", "example.com, EXAMPLE.ORG , test.net") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.local_domains, vec!["example.com", "example.org", "test.net"]);
    clear_mailrs_env();
}

#[test]
fn from_env_local_domains_lowercased() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_LOCAL_DOMAINS", "UPPER.COM") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.local_domains, vec!["upper.com"]);
    clear_mailrs_env();
}

#[test]
fn from_env_dnsbl_zones() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DNSBL_ZONES", "zen.spamhaus.org, bl.spamcop.net") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.dnsbl_zones, vec!["zen.spamhaus.org", "bl.spamcop.net"]);
    clear_mailrs_env();
}

#[test]
fn from_env_acme_domains() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_ACME_DOMAINS", "mx.example.com, mail.example.com") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.acme_domains, vec!["mx.example.com", "mail.example.com"]);
    clear_mailrs_env();
}

#[test]
fn from_env_mta_sts_mx() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_MTA_STS_MX", "mx1.example.com, mx2.example.com") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.mta_sts_mx, vec!["mx1.example.com", "mx2.example.com"]);
    clear_mailrs_env();
}

// =====================================================================
// from_env — boolean parsing
// =====================================================================

#[test]
fn from_env_dnsbl_enabled_false() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DNSBL_ENABLED", "false") };
    let cfg = ServerConfig::from_env();
    assert!(!cfg.dnsbl_enabled);
    clear_mailrs_env();
}

#[test]
fn from_env_dnsbl_enabled_zero() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DNSBL_ENABLED", "0") };
    let cfg = ServerConfig::from_env();
    assert!(!cfg.dnsbl_enabled);
    clear_mailrs_env();
}

#[test]
fn from_env_dnsbl_enabled_true_value() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DNSBL_ENABLED", "1") };
    let cfg = ServerConfig::from_env();
    assert!(cfg.dnsbl_enabled);
    clear_mailrs_env();
}

#[test]
fn from_env_dnsbl_enabled_case_insensitive_false() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DNSBL_ENABLED", "FALSE") };
    let cfg = ServerConfig::from_env();
    assert!(!cfg.dnsbl_enabled);
    clear_mailrs_env();
}

#[test]
fn from_env_dnsbl_enabled_any_string_is_true() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DNSBL_ENABLED", "yes") };
    let cfg = ServerConfig::from_env();
    assert!(cfg.dnsbl_enabled);
    clear_mailrs_env();
}

#[test]
fn from_env_antispam_enabled_false() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_ANTISPAM_ENABLED", "false") };
    let cfg = ServerConfig::from_env();
    assert!(!cfg.antispam_enabled);
    clear_mailrs_env();
}

#[test]
fn from_env_spf_enabled_backwards_compat() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    // MAILRS_SPF_ENABLED is the legacy name
    unsafe { std::env::set_var("MAILRS_SPF_ENABLED", "0") };
    let cfg = ServerConfig::from_env();
    assert!(!cfg.antispam_enabled);
    clear_mailrs_env();
}

#[test]
fn from_env_antispam_takes_precedence_over_spf() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_ANTISPAM_ENABLED", "1") };
    unsafe { std::env::set_var("MAILRS_SPF_ENABLED", "0") };
    let cfg = ServerConfig::from_env();
    // MAILRS_ANTISPAM_ENABLED is checked first, so SPF fallback is skipped
    assert!(cfg.antispam_enabled);
    clear_mailrs_env();
}

#[test]
fn from_env_acme_staging_true() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_ACME_STAGING", "true") };
    let cfg = ServerConfig::from_env();
    assert!(cfg.acme_staging);
    clear_mailrs_env();
}

#[test]
fn from_env_acme_staging_one() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_ACME_STAGING", "1") };
    let cfg = ServerConfig::from_env();
    assert!(cfg.acme_staging);
    clear_mailrs_env();
}

#[test]
fn from_env_acme_staging_false_for_other_values() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_ACME_STAGING", "yes") };
    let cfg = ServerConfig::from_env();
    // only "1" and "true" (case-insensitive) enable it
    assert!(!cfg.acme_staging);
    clear_mailrs_env();
}

#[test]
fn from_env_ai_analysis_enabled() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AI_ANALYSIS_ENABLED", "true") };
    let cfg = ServerConfig::from_env();
    assert!(cfg.ai_analysis_enabled);
    clear_mailrs_env();
}

#[test]
fn from_env_ai_analysis_enabled_one() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AI_ANALYSIS_ENABLED", "1") };
    let cfg = ServerConfig::from_env();
    assert!(cfg.ai_analysis_enabled);
    clear_mailrs_env();
}

// =====================================================================
// from_env — smuggle protection
// =====================================================================

#[test]
fn from_env_smuggle_protection_strict() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_SMUGGLE_PROTECTION", "strict") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smuggle_protection, SmuggleProtection::Strict);
    clear_mailrs_env();
}

#[test]
fn from_env_smuggle_protection_off() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_SMUGGLE_PROTECTION", "off") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smuggle_protection, SmuggleProtection::Off);
    clear_mailrs_env();
}

#[test]
fn from_env_smuggle_protection_permissive_explicit() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_SMUGGLE_PROTECTION", "permissive") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smuggle_protection, SmuggleProtection::Permissive);
    clear_mailrs_env();
}

#[test]
fn from_env_smuggle_protection_unknown_defaults_permissive() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_SMUGGLE_PROTECTION", "unknown_value") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smuggle_protection, SmuggleProtection::Permissive);
    clear_mailrs_env();
}

#[test]
fn from_env_smuggle_protection_case_insensitive() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_SMUGGLE_PROTECTION", "STRICT") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.smuggle_protection, SmuggleProtection::Strict);
    clear_mailrs_env();
}

// =====================================================================
// from_env — DKIM fields (empty string handling)
// =====================================================================

#[test]
fn from_env_dkim_selector_non_empty() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DKIM_SELECTOR", "default") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.dkim_selector, Some("default".into()));
    clear_mailrs_env();
}

#[test]
fn from_env_dkim_selector_empty_string_stays_none() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DKIM_SELECTOR", "") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.dkim_selector, None);
    clear_mailrs_env();
}

#[test]
fn from_env_dkim_domain_non_empty() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DKIM_DOMAIN", "example.com") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.dkim_domain, Some("example.com".into()));
    clear_mailrs_env();
}

#[test]
fn from_env_dkim_domain_empty_stays_none() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DKIM_DOMAIN", "") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.dkim_domain, None);
    clear_mailrs_env();
}

#[test]
fn from_env_dkim_private_key_non_empty() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DKIM_PRIVATE_KEY", "/etc/dkim/key.pem") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.dkim_private_key_path, Some(PathBuf::from("/etc/dkim/key.pem")));
    clear_mailrs_env();
}

#[test]
fn from_env_dkim_private_key_empty_stays_none() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_DKIM_PRIVATE_KEY", "") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.dkim_private_key_path, None);
    clear_mailrs_env();
}

// =====================================================================
// from_env — ACME fields
// =====================================================================

#[test]
fn from_env_acme_email_non_empty() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_ACME_EMAIL", "admin@example.com") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.acme_email, Some("admin@example.com".into()));
    clear_mailrs_env();
}

#[test]
fn from_env_acme_email_empty_stays_none() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_ACME_EMAIL", "") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.acme_email, None);
    clear_mailrs_env();
}

#[test]
fn from_env_acme_dir() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_ACME_DIR", "/custom/acme") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.acme_dir, PathBuf::from("/custom/acme"));
    clear_mailrs_env();
}

// =====================================================================
// from_env — numeric fields
// =====================================================================

#[test]
fn from_env_rate_limit_capacity() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_RATE_LIMIT_CAPACITY", "50") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.rate_limit_capacity, 50);
    clear_mailrs_env();
}

#[test]
fn from_env_rate_limit_refill() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_RATE_LIMIT_REFILL", "2.5") };
    let cfg = ServerConfig::from_env();
    assert!((cfg.rate_limit_refill - 2.5).abs() < f64::EPSILON);
    clear_mailrs_env();
}

#[test]
fn from_env_greylist_delay() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_GREYLIST_DELAY", "600") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.greylist_delay_secs, 600);
    clear_mailrs_env();
}

#[test]
fn from_env_spam_score_threshold() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_SPAM_SCORE_THRESHOLD", "3.5") };
    let cfg = ServerConfig::from_env();
    assert!((cfg.spam_score_threshold - 3.5).abs() < f64::EPSILON);
    clear_mailrs_env();
}

#[test]
fn from_env_mta_sts_max_age() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_MTA_STS_MAX_AGE", "86400") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.mta_sts_max_age, 86400);
    clear_mailrs_env();
}

#[test]
fn from_env_mta_sts_id() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_MTA_STS_ID", "20260101000000") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.mta_sts_id, "20260101000000");
    clear_mailrs_env();
}

#[test]
fn from_env_mta_sts_mode() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_MTA_STS_MODE", "enforce") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.mta_sts_mode, Some("enforce".into()));
    clear_mailrs_env();
}

#[test]
fn from_env_invalid_numeric_keeps_default() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_RATE_LIMIT_CAPACITY", "abc") };
    unsafe { std::env::set_var("MAILRS_GREYLIST_DELAY", "xyz") };
    unsafe { std::env::set_var("MAILRS_SPAM_SCORE_THRESHOLD", "not_float") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.rate_limit_capacity, 10);
    assert_eq!(cfg.greylist_delay_secs, 300);
    assert!((cfg.spam_score_threshold - 5.0).abs() < f64::EPSILON);
    clear_mailrs_env();
}

// =====================================================================
// from_env — LLM / ClamAV
// =====================================================================

#[test]
fn from_env_llm_url() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_LLM_URL", "https://custom.llm/api") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.llm_url, "https://custom.llm/api");
    clear_mailrs_env();
}

#[test]
fn from_env_clamav_addr() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_CLAMAV_ADDR", "127.0.0.1:3310") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.clamav_addr, Some("127.0.0.1:3310".into()));
    clear_mailrs_env();
}

// =====================================================================
// from_env — auth guard
// =====================================================================

#[test]
fn from_env_auth_max_failures() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AUTH_MAX_FAILURES", "10") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.auth_max_failures_account, 10);
    clear_mailrs_env();
}

#[test]
fn from_env_auth_account_window_secs() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AUTH_ACCOUNT_WINDOW_SECS", "1800") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.auth_account_window_secs, 1800);
    clear_mailrs_env();
}

#[test]
fn from_env_auth_lockout_secs() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AUTH_LOCKOUT_SECS", "3600") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.auth_base_lockout_secs, 3600);
    clear_mailrs_env();
}

#[test]
fn from_env_auth_max_failures_ip() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AUTH_MAX_FAILURES_IP", "50") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.auth_max_failures_ip, 50);
    clear_mailrs_env();
}

#[test]
fn from_env_auth_ip_window_secs() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AUTH_IP_WINDOW_SECS", "7200") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.auth_ip_window_secs, 7200);
    clear_mailrs_env();
}

#[test]
fn from_env_auth_ip_lockout_secs() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AUTH_IP_LOCKOUT_SECS", "7200") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.auth_ip_base_lockout_secs, 7200);
    clear_mailrs_env();
}

#[test]
fn from_env_auth_backoff_multiplier() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AUTH_BACKOFF_MULTIPLIER", "3.0") };
    let cfg = ServerConfig::from_env();
    assert!((cfg.auth_backoff_multiplier - 3.0).abs() < f64::EPSILON);
    clear_mailrs_env();
}

#[test]
fn from_env_auth_max_lockout_secs() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_AUTH_MAX_LOCKOUT_SECS", "172800") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.auth_max_lockout_secs, 172800);
    clear_mailrs_env();
}

// =====================================================================
// from_env — storage backends
// =====================================================================

#[test]
fn from_env_pg_url() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_PG_URL", "postgres://user:pass@localhost/mailrs") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.pg_url, Some("postgres://user:pass@localhost/mailrs".into()));
    clear_mailrs_env();
}

#[test]
fn from_env_valkey_url() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_VALKEY_URL", "redis://localhost:6379") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.valkey_url, Some("redis://localhost:6379".into()));
    clear_mailrs_env();
}

// =====================================================================
// from_env — web static dir
// =====================================================================

#[test]
fn from_env_web_static_dir() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_WEB_STATIC_DIR", "/var/www/mailrs") };
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.web_static_dir, Some(PathBuf::from("/var/www/mailrs")));
    clear_mailrs_env();
}

// =====================================================================
// from_env — clean environment returns defaults
// =====================================================================

#[test]
fn from_env_clean_returns_defaults() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.hostname, "mx.mailrs.local");
    assert_eq!(cfg.maildir_root, "/tmp/mailrs");
    assert_eq!(cfg.smtp_port, 2525);
    assert_eq!(cfg.submission_port, 2587);
    assert_eq!(cfg.web_port, 3100);
    assert!(cfg.local_domains.is_empty());
    assert!(cfg.tls_cert.is_none());
    assert!(cfg.tls_key.is_none());
    assert!(!cfg.ai_analysis_enabled);
    clear_mailrs_env();
}

// =====================================================================
// from_env — full integration: set many env vars at once
// =====================================================================

#[test]
fn from_env_full_configuration() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    unsafe { std::env::set_var("MAILRS_HOSTNAME", "mx.prod.com") };
    unsafe { std::env::set_var("MAILRS_MAILDIR", "/data/mail") };
    unsafe { std::env::set_var("MAILRS_PORT", "25") };
    unsafe { std::env::set_var("MAILRS_SUBMISSION_PORT", "587") };
    unsafe { std::env::set_var("MAILRS_SMTPS_PORT", "465") };
    unsafe { std::env::set_var("MAILRS_WEB_PORT", "443") };
    unsafe { std::env::set_var("MAILRS_IMAP_PORT", "143") };
    unsafe { std::env::set_var("MAILRS_IMAPS_PORT", "993") };
    unsafe { std::env::set_var("MAILRS_LOCAL_DOMAINS", "prod.com,prod.org") };
    unsafe { std::env::set_var("MAILRS_TLS_CERT", "/ssl/cert.pem") };
    unsafe { std::env::set_var("MAILRS_TLS_KEY", "/ssl/key.pem") };
    unsafe { std::env::set_var("MAILRS_DNSBL_ENABLED", "false") };
    unsafe { std::env::set_var("MAILRS_SMUGGLE_PROTECTION", "strict") };
    unsafe { std::env::set_var("MAILRS_DKIM_SELECTOR", "mail") };
    unsafe { std::env::set_var("MAILRS_DKIM_DOMAIN", "prod.com") };
    unsafe { std::env::set_var("MAILRS_DKIM_PRIVATE_KEY", "/dkim/key.pem") };
    unsafe { std::env::set_var("MAILRS_PG_URL", "postgres://localhost/mail") };
    unsafe { std::env::set_var("MAILRS_VALKEY_URL", "redis://localhost:6379") };

    let cfg = ServerConfig::from_env();
    assert_eq!(cfg.hostname, "mx.prod.com");
    assert_eq!(cfg.maildir_root, "/data/mail");
    assert_eq!(cfg.smtp_port, 25);
    assert_eq!(cfg.submission_port, 587);
    assert_eq!(cfg.smtps_port, 465);
    assert_eq!(cfg.web_port, 443);
    assert_eq!(cfg.imap_port, 143);
    assert_eq!(cfg.imaps_port, 993);
    assert_eq!(cfg.local_domains, vec!["prod.com", "prod.org"]);
    assert_eq!(cfg.tls_cert, Some(PathBuf::from("/ssl/cert.pem")));
    assert_eq!(cfg.tls_key, Some(PathBuf::from("/ssl/key.pem")));
    assert!(!cfg.dnsbl_enabled);
    assert_eq!(cfg.smuggle_protection, SmuggleProtection::Strict);
    assert_eq!(cfg.dkim_selector, Some("mail".into()));
    assert_eq!(cfg.dkim_domain, Some("prod.com".into()));
    assert_eq!(cfg.dkim_private_key_path, Some(PathBuf::from("/dkim/key.pem")));
    assert_eq!(cfg.pg_url, Some("postgres://localhost/mail".into()));
    assert_eq!(cfg.valkey_url, Some("redis://localhost:6379".into()));

    clear_mailrs_env();
}

// =====================================================================
// SmuggleProtection enum equality
// =====================================================================

#[test]
#[allow(clippy::clone_on_copy)]
fn smuggle_protection_clone_and_copy() {
    let sp = SmuggleProtection::Strict;
    let cloned = sp.clone();
    let copied = sp;
    assert_eq!(sp, cloned);
    assert_eq!(sp, copied);
}

#[test]
fn smuggle_protection_debug() {
    assert_eq!(format!("{:?}", SmuggleProtection::Strict), "Strict");
    assert_eq!(format!("{:?}", SmuggleProtection::Permissive), "Permissive");
    assert_eq!(format!("{:?}", SmuggleProtection::Off), "Off");
}

// =====================================================================
// TlsMode enum
// =====================================================================

#[test]
fn tls_mode_debug() {
    assert_eq!(format!("{:?}", TlsMode::None), "None");
    assert_eq!(format!("{:?}", TlsMode::Manual), "Manual");
    assert_eq!(format!("{:?}", TlsMode::Acme), "Acme");
}

#[test]
fn tls_mode_equality() {
    assert_eq!(TlsMode::None, TlsMode::None);
    assert_eq!(TlsMode::Manual, TlsMode::Manual);
    assert_eq!(TlsMode::Acme, TlsMode::Acme);
    assert_ne!(TlsMode::None, TlsMode::Manual);
    assert_ne!(TlsMode::Manual, TlsMode::Acme);
    assert_ne!(TlsMode::Acme, TlsMode::None);
}

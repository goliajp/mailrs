//! Tests for `system_config` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn default_runtime_config_matches_server_config_defaults() {
    let rc = RuntimeConfig::default();
    assert!(!rc.ai_analysis_enabled);
    assert!(rc.antispam_enabled);
    assert_eq!(rc.spam_score_threshold, 5.0);
    assert_eq!(rc.smuggle_protection, "permissive");
    assert!(rc.webhook_url.is_none());
}

#[test]
fn apply_value_sets_fields() {
    let mut cfg = RuntimeConfig::default();

    apply_value(&mut cfg, "webhook_url", "https://example.com/hook");
    assert_eq!(cfg.webhook_url, Some("https://example.com/hook".into()));

    apply_value(&mut cfg, "ai_analysis_enabled", "true");
    assert!(cfg.ai_analysis_enabled);

    apply_value(&mut cfg, "spam_score_threshold", "7.5");
    assert_eq!(cfg.spam_score_threshold, 7.5);

    apply_value(&mut cfg, "smuggle_protection", "strict");
    assert_eq!(cfg.smuggle_protection, "strict");
}

#[test]
fn apply_value_empty_string_clears_optional() {
    let mut cfg = RuntimeConfig {
        webhook_url: Some("https://example.com".into()),
        ..RuntimeConfig::default()
    };
    apply_value(&mut cfg, "webhook_url", "");
    assert!(cfg.webhook_url.is_none());
}

#[test]
fn apply_value_invalid_f64_is_ignored() {
    let mut cfg = RuntimeConfig::default();
    apply_value(&mut cfg, "spam_score_threshold", "not_a_number");
    assert_eq!(cfg.spam_score_threshold, 5.0); // unchanged
}

#[test]
fn get_value_round_trips() {
    let cfg = RuntimeConfig {
        webhook_url: Some("https://test.com".into()),
        ai_analysis_enabled: true,
        spam_score_threshold: 3.5,
        smuggle_protection: "strict".into(),
        ..RuntimeConfig::default()
    };
    assert_eq!(get_value(&cfg, "webhook_url"), "https://test.com");
    assert_eq!(get_value(&cfg, "ai_analysis_enabled"), "true");
    assert_eq!(get_value(&cfg, "spam_score_threshold"), "3.5");
    assert_eq!(get_value(&cfg, "smuggle_protection"), "strict");
}

#[test]
fn validate_value_bool() {
    let info = find_key("ai_analysis_enabled").unwrap();
    assert!(validate_value(info, "true").is_ok());
    assert!(validate_value(info, "false").is_ok());
    assert!(validate_value(info, "yes").is_err());
}

#[test]
fn validate_value_f64() {
    let info = find_key("spam_score_threshold").unwrap();
    assert!(validate_value(info, "5.0").is_ok());
    assert!(validate_value(info, "0").is_ok());
    assert!(validate_value(info, "abc").is_err());
}

#[test]
fn validate_value_enum() {
    let info = find_key("smuggle_protection").unwrap();
    assert!(validate_value(info, "strict").is_ok());
    assert!(validate_value(info, "permissive").is_ok());
    assert!(validate_value(info, "off").is_ok());
    assert!(validate_value(info, "invalid").is_err());
}

#[test]
fn validate_value_string_accepts_anything() {
    let info = find_key("webhook_url").unwrap();
    assert!(validate_value(info, "").is_ok());
    assert!(validate_value(info, "https://example.com").is_ok());
}

#[test]
fn mask_sensitive_values() {
    assert_eq!(mask_sensitive(""), "");
    assert_eq!(mask_sensitive("abc"), "****");
    assert_eq!(mask_sensitive("abcd"), "****");
    assert_eq!(mask_sensitive("abcde"), "abcd****");
    assert_eq!(mask_sensitive("sk-1234567890"), "sk-1****");
}

#[test]
fn find_key_returns_correct_info() {
    let info = find_key("webhook_url").unwrap();
    assert_eq!(info.group, "webhook");
    assert!(!info.sensitive);

    let info = find_key("llm_api_key").unwrap();
    assert!(info.sensitive);

    assert!(find_key("nonexistent").is_none());
}

#[test]
fn config_keys_covers_all_phase1_fields() {
    let keys: Vec<&str> = CONFIG_KEYS.iter().map(|k| k.key).collect();
    assert!(keys.contains(&"webhook_url"));
    assert!(keys.contains(&"webhook_api_key"));
    assert!(keys.contains(&"ai_analysis_enabled"));
    assert!(keys.contains(&"llm_url"));
    assert!(keys.contains(&"llm_api_key"));
    assert!(keys.contains(&"antispam_enabled"));
    assert!(keys.contains(&"spam_score_threshold"));
    assert!(keys.contains(&"smuggle_protection"));
    assert_eq!(keys.len(), 8);
}

#[test]
fn smuggle_protection_enum_conversion() {
    let mut cfg = RuntimeConfig::default();
    assert_eq!(cfg.smuggle_protection_enum(), SmuggleProtection::Permissive);

    cfg.smuggle_protection = "strict".into();
    assert_eq!(cfg.smuggle_protection_enum(), SmuggleProtection::Strict);

    cfg.smuggle_protection = "off".into();
    assert_eq!(cfg.smuggle_protection_enum(), SmuggleProtection::Off);

    cfg.smuggle_protection = "unknown".into();
    assert_eq!(cfg.smuggle_protection_enum(), SmuggleProtection::Permissive);
}

#[test]
fn store_new_starts_with_env_defaults() {
    let env = RuntimeConfig {
        webhook_url: Some("https://env.example.com".into()),
        ..RuntimeConfig::default()
    };
    let store = SystemConfigStore::new(None, None, env);
    let cfg = store.get();
    assert_eq!(cfg.webhook_url, Some("https://env.example.com".into()));
}

//! Env-var → field loaders for [`super::ServerConfig`].
//!
//! Split out of `config/mod.rs` to keep that file under the 500-LOC
//! limit. Each `load_*` method maps one logical section of
//! `MAILRS_*` environment variables onto the corresponding struct
//! fields. They are called in fixed order by `ServerConfig::from_env`
//! so subsequent loaders can rely on previously-set values if needed.

use std::path::PathBuf;

use mailrs_smtp_codec::SmuggleProtection;

use super::ServerConfig;

// ---------- env-reader primitives (each does one thing) ----------

/// Overwrite `target` with `env[name]` if the var is set. Empty
/// values *are* honored — useful for tests that want to clear a
/// value back to "" by setting the env var to empty.
fn set_string(name: &str, target: &mut String) {
    if let Ok(v) = std::env::var(name) {
        *target = v;
    }
}

/// Overwrite `target` with `Some(env[name])` if the var is set.
/// Empty values still produce `Some("")`.
fn set_opt_string(name: &str, target: &mut Option<String>) {
    if let Ok(v) = std::env::var(name) {
        *target = Some(v);
    }
}

/// Like [`set_opt_string`] but skips empty values (env unset OR
/// empty string both leave `target` unchanged). Used for secrets
/// and identifiers where empty has the same meaning as "absent".
fn set_opt_string_nonempty(name: &str, target: &mut Option<String>) {
    if let Ok(v) = std::env::var(name)
        && !v.is_empty()
    {
        *target = Some(v);
    }
}

/// Overwrite `target` with `Some(PathBuf::from(env[name]))` if
/// the var is set. Empty values are honored (becomes `Some("")`).
fn set_opt_path(name: &str, target: &mut Option<PathBuf>) {
    if let Ok(v) = std::env::var(name) {
        *target = Some(PathBuf::from(v));
    }
}

/// Parse `env[name]` as `T` (via `FromStr`); on success overwrite
/// `target`. Unparseable values are silently ignored (preserving
/// the previous default) — matches the original behavior.
fn set_parsed<T: std::str::FromStr>(name: &str, target: &mut T) {
    if let Ok(v) = std::env::var(name)
        && let Ok(parsed) = v.parse::<T>()
    {
        *target = parsed;
    }
}

/// Split `env[name]` on `,`, trim each piece, lowercase, collect
/// into `target`. Used for domain lists.
fn set_csv_lower(name: &str, target: &mut Vec<String>) {
    if let Ok(v) = std::env::var(name) {
        *target = v.split(',').map(|s| s.trim().to_lowercase()).collect();
    }
}

/// Split `env[name]` on `,`, trim each piece, collect into
/// `target`. Used for case-sensitive lists (DNSBL zones, MX
/// hostnames, etc.).
fn set_csv(name: &str, target: &mut Vec<String>) {
    if let Ok(v) = std::env::var(name) {
        *target = v.split(',').map(|s| s.trim().to_string()).collect();
    }
}

/// Boolean with "truthy/falsy" semantics: any value other than
/// `"0"` / case-insensitive `"false"` is true. Used for toggles
/// where presence-without-value should enable the feature.
fn set_bool_truthy(name: &str, target: &mut bool) {
    if let Ok(v) = std::env::var(name) {
        *target = v != "0" && v.to_lowercase() != "false";
    }
}

/// Boolean with explicit "1/true" semantics: only `"1"` or
/// case-insensitive `"true"` enable. Anything else (including
/// presence-without-value `""`) leaves the field unchanged.
fn set_bool_loose(name: &str, target: &mut bool) {
    if let Ok(v) = std::env::var(name) {
        *target = v == "1" || v.to_lowercase() == "true";
    }
}

impl ServerConfig {
    /// Network listeners + hostname + maildir root + local domains.
    pub(super) fn load_network(&mut self) {
        set_string("MAILRS_HOSTNAME", &mut self.hostname);
        set_string("MAILRS_MAILDIR", &mut self.maildir_root);
        set_parsed("MAILRS_PORT", &mut self.smtp_port);
        set_parsed("MAILRS_SUBMISSION_PORT", &mut self.submission_port);
        set_parsed("MAILRS_SMTPS_PORT", &mut self.smtps_port);
        set_parsed("MAILRS_WEB_PORT", &mut self.web_port);
        set_parsed("MAILRS_IMAP_PORT", &mut self.imap_port);
        set_parsed("MAILRS_IMAPS_PORT", &mut self.imaps_port);
        set_bool_truthy("MAILRS_DISABLE_PLAIN_IMAP", &mut self.disable_plain_imap);
        set_parsed("MAILRS_POP3_PORT", &mut self.pop3_port);
        set_bool_truthy("MAILRS_DISABLE_PLAIN_POP3", &mut self.disable_plain_pop3);
        set_parsed("MAILRS_MANAGESIEVE_PORT", &mut self.managesieve_port);
        set_csv_lower("MAILRS_LOCAL_DOMAINS", &mut self.local_domains);
        set_opt_path("MAILRS_WEB_STATIC_DIR", &mut self.web_static_dir);
        set_opt_path("MAILRS_USERS_FILE", &mut self.users_file);
    }

    /// PostgreSQL + Valkey URLs.
    pub(super) fn load_storage(&mut self) {
        set_opt_string("MAILRS_PG_URL", &mut self.pg_url);
        set_opt_string("MAILRS_VALKEY_URL", &mut self.valkey_url);
    }

    /// Manual TLS cert + key paths (vs ACME auto-issued).
    pub(super) fn load_tls(&mut self) {
        set_opt_path("MAILRS_TLS_CERT", &mut self.tls_cert);
        set_opt_path("MAILRS_TLS_KEY", &mut self.tls_key);
    }

    /// ACME (Let's Encrypt) automatic cert issuance.
    pub(super) fn load_acme(&mut self) {
        set_opt_string_nonempty("MAILRS_ACME_EMAIL", &mut self.acme_email);
        set_csv("MAILRS_ACME_DOMAINS", &mut self.acme_domains);
        if let Ok(v) = std::env::var("MAILRS_ACME_DIR") {
            self.acme_dir = PathBuf::from(v);
        }
        set_bool_loose("MAILRS_ACME_STAGING", &mut self.acme_staging);
    }

    /// Inbound rate-limit + DNSBL + greylist + anti-spam toggle +
    /// smuggle-protection mode.
    pub(super) fn load_inbound_policy(&mut self) {
        set_csv("MAILRS_DNSBL_ZONES", &mut self.dnsbl_zones);
        set_parsed("MAILRS_RATE_LIMIT_CAPACITY", &mut self.rate_limit_capacity);
        set_parsed("MAILRS_RATE_LIMIT_REFILL", &mut self.rate_limit_refill);
        set_parsed("MAILRS_GREYLIST_DELAY", &mut self.greylist_delay_secs);
        set_bool_truthy("MAILRS_DNSBL_ENABLED", &mut self.dnsbl_enabled);
        // backwards-compatible alias: MAILRS_SPF_ENABLED → antispam_enabled
        if let Ok(v) = std::env::var("MAILRS_ANTISPAM_ENABLED") {
            self.antispam_enabled = v != "0" && v.to_lowercase() != "false";
        } else if let Ok(v) = std::env::var("MAILRS_SPF_ENABLED") {
            self.antispam_enabled = v != "0" && v.to_lowercase() != "false";
        }
        if let Ok(v) = std::env::var("MAILRS_SMUGGLE_PROTECTION") {
            self.smuggle_protection = match v.to_lowercase().as_str() {
                "strict" => SmuggleProtection::Strict,
                "off" => SmuggleProtection::Off,
                _ => SmuggleProtection::Permissive,
            };
        }
    }

    /// DKIM outbound signing config (selector + d= domain + key).
    pub(super) fn load_dkim_signing(&mut self) {
        set_opt_string_nonempty("MAILRS_DKIM_SELECTOR", &mut self.dkim_selector);
        set_opt_string_nonempty("MAILRS_DKIM_DOMAIN", &mut self.dkim_domain);
        if let Ok(v) = std::env::var("MAILRS_DKIM_PRIVATE_KEY")
            && !v.is_empty()
        {
            self.dkim_private_key_path = Some(PathBuf::from(v));
        }
    }

    /// MTA-STS publishing config (mode + MX list + max-age + id).
    pub(super) fn load_mta_sts(&mut self) {
        set_opt_string("MAILRS_MTA_STS_MODE", &mut self.mta_sts_mode);
        set_csv("MAILRS_MTA_STS_MX", &mut self.mta_sts_mx);
        set_parsed("MAILRS_MTA_STS_MAX_AGE", &mut self.mta_sts_max_age);
        set_string("MAILRS_MTA_STS_ID", &mut self.mta_sts_id);
    }

    /// Spam / ClamAV / LLM / AI-analysis + SRS secret.
    pub(super) fn load_anti_abuse(&mut self) {
        set_parsed(
            "MAILRS_SPAM_SCORE_THRESHOLD",
            &mut self.spam_score_threshold,
        );
        set_opt_string("MAILRS_CLAMAV_ADDR", &mut self.clamav_addr);
        set_string("MAILRS_LLM_URL", &mut self.llm_url);
        set_opt_string_nonempty("MAILRS_LLM_API_KEY", &mut self.llm_api_key);
        if std::env::var("MAILRS_AI_ANALYSIS_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
        {
            self.ai_analysis_enabled = true;
        }
        set_opt_string_nonempty("MAILRS_SRS_SECRET", &mut self.srs_secret);
    }

    /// Brute-force auth guard thresholds (per-account + per-IP).
    pub(super) fn load_auth_guard(&mut self) {
        set_parsed(
            "MAILRS_AUTH_MAX_FAILURES",
            &mut self.auth_max_failures_account,
        );
        set_parsed(
            "MAILRS_AUTH_ACCOUNT_WINDOW_SECS",
            &mut self.auth_account_window_secs,
        );
        set_parsed("MAILRS_AUTH_LOCKOUT_SECS", &mut self.auth_base_lockout_secs);
        set_parsed(
            "MAILRS_AUTH_MAX_FAILURES_IP",
            &mut self.auth_max_failures_ip,
        );
        set_parsed("MAILRS_AUTH_IP_WINDOW_SECS", &mut self.auth_ip_window_secs);
        set_parsed(
            "MAILRS_AUTH_IP_LOCKOUT_SECS",
            &mut self.auth_ip_base_lockout_secs,
        );
        set_parsed(
            "MAILRS_AUTH_BACKOFF_MULTIPLIER",
            &mut self.auth_backoff_multiplier,
        );
        set_parsed(
            "MAILRS_AUTH_MAX_LOCKOUT_SECS",
            &mut self.auth_max_lockout_secs,
        );
    }

    /// Chrome CDP for HTML preview + Meilisearch full-text index.
    pub(super) fn load_external_services(&mut self) {
        set_opt_string_nonempty("MAILRS_CHROME_CDP_URL", &mut self.chrome_cdp_url);
        set_opt_string_nonempty("MAILRS_MEILI_URL", &mut self.meili_url);
        set_opt_string_nonempty("MAILRS_MEILI_KEY", &mut self.meili_key);
    }

    /// LDAP fallback-auth backend.
    pub(super) fn load_ldap(&mut self) {
        set_opt_string_nonempty("MAILRS_LDAP_URL", &mut self.ldap_url);
        set_opt_string_nonempty("MAILRS_LDAP_BIND_DN", &mut self.ldap_bind_dn);
        set_opt_string_nonempty("MAILRS_LDAP_BIND_PASSWORD", &mut self.ldap_bind_password);
        set_opt_string_nonempty("MAILRS_LDAP_BASE_DN", &mut self.ldap_base_dn);
        set_opt_string_nonempty("MAILRS_LDAP_USER_FILTER", &mut self.ldap_user_filter);
    }

    /// Global webhook (every email event → POST <url>).
    pub(super) fn load_webhook(&mut self) {
        set_opt_string_nonempty("MAILRS_WEBHOOK_URL", &mut self.webhook_url);
        set_opt_string_nonempty("MAILRS_WEBHOOK_API_KEY", &mut self.webhook_api_key);
    }
}

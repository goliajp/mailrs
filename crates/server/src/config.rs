//! Server configuration loaded from `MAILRS_*` environment variables.
//!
//! ## Tests, `ENV_LOCK`, and `unsafe { set_var }`
//!
//! Rust 2024 marked `std::env::set_var` / `remove_var` as `unsafe` because
//! mutating the process environment while other threads might read it is a
//! data race per POSIX. In production this never happens — `from_env`
//! reads each variable once at startup and never writes.
//!
//! In the test module, however, we mutate `MAILRS_*` per test to exercise
//! `from_env` behavior, so every test goes through a single
//! `ENV_LOCK: Mutex<()>` (see `mod tests`) before any `set_var`/`remove_var`.
//! That serializes the env-touching tests across threads, which is what
//! makes the `unsafe { … }` calls sound: only one thread can be inside the
//! mutex at a time, and no production thread reads env after startup. Tests
//! that never touch the environment do not need to hold the lock.

use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub enum TlsMode {
    Acme,
    Manual,
    None,
}

// `SmuggleProtection` moved to the `mailrs-smtp-codec` stone in
// 2026-05-24. Re-exported here so existing `crate::config::SmuggleProtection`
// imports across the server keep working — new code should import
// directly from `mailrs_smtp_codec`.
pub use mailrs_smtp_codec::SmuggleProtection;

pub struct ServerConfig {
    pub hostname: String,
    pub maildir_root: String,
    pub smtp_port: u16,
    pub submission_port: u16,
    pub smtps_port: u16,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub users_file: Option<PathBuf>,
    pub web_port: u16,
    // IMAP
    pub imap_port: u16,
    pub imaps_port: u16,
    // POP3
    pub pop3_port: u16,
    // ManageSieve
    pub managesieve_port: u16,
    // anti-spam
    pub local_domains: Vec<String>,
    pub dnsbl_zones: Vec<String>,
    pub rate_limit_capacity: u32,
    pub rate_limit_refill: f64,
    pub greylist_delay_secs: u64,
    pub dnsbl_enabled: bool,
    pub antispam_enabled: bool,
    // DKIM signing
    pub dkim_selector: Option<String>,
    pub dkim_domain: Option<String>,
    pub dkim_private_key_path: Option<PathBuf>,
    // smuggling protection
    pub smuggle_protection: SmuggleProtection,
    // web frontend
    pub web_static_dir: Option<PathBuf>,
    // ACME
    pub acme_email: Option<String>,
    pub acme_domains: Vec<String>,
    pub acme_dir: PathBuf,
    pub acme_staging: bool,
    // MTA-STS
    pub mta_sts_mode: Option<String>,
    pub mta_sts_mx: Vec<String>,
    pub mta_sts_max_age: u64,
    pub mta_sts_id: String,
    // spam filtering
    pub spam_score_threshold: f64,
    // ClamAV
    pub clamav_addr: Option<String>,
    // AI email analysis (self-hosted LLM)
    pub llm_url: String,
    pub llm_api_key: Option<String>,
    pub ai_analysis_enabled: bool,
    // auth guard (brute force protection)
    pub auth_max_failures_account: u32,
    pub auth_account_window_secs: u64,
    pub auth_base_lockout_secs: u64,
    pub auth_max_failures_ip: u32,
    pub auth_ip_window_secs: u64,
    pub auth_ip_base_lockout_secs: u64,
    pub auth_backoff_multiplier: f64,
    pub auth_max_lockout_secs: u64,
    // SRS (Sender Rewriting Scheme)
    pub srs_secret: Option<String>,
    // storage backends
    pub pg_url: Option<String>,
    pub valkey_url: Option<String>,
    // Meilisearch
    pub meili_url: Option<String>,
    pub meili_key: Option<String>,
    // Chrome CDP (headless browser for email rendering preview)
    pub chrome_cdp_url: Option<String>,
    // LDAP authentication
    pub ldap_url: Option<String>,
    pub ldap_bind_dn: Option<String>,
    pub ldap_bind_password: Option<String>,
    pub ldap_base_dn: Option<String>,
    pub ldap_user_filter: Option<String>,
    // global webhook (fire-and-forget POST on new mail)
    pub webhook_url: Option<String>,
    pub webhook_api_key: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            hostname: "mx.mailrs.local".into(),
            maildir_root: "/tmp/mailrs".into(),
            smtp_port: 2525,
            submission_port: 2587,
            smtps_port: 2465,
            tls_cert: None,
            tls_key: None,
            users_file: None,
            web_port: 3100,
            imap_port: 1143,
            imaps_port: 1993,
            pop3_port: 1110,
            managesieve_port: 4190,
            local_domains: vec![],
            dnsbl_zones: vec![],
            rate_limit_capacity: 10,
            rate_limit_refill: 1.0,
            greylist_delay_secs: 300,
            dnsbl_enabled: true,
            antispam_enabled: true,
            dkim_selector: None,
            dkim_domain: None,
            dkim_private_key_path: None,
            smuggle_protection: SmuggleProtection::Permissive,
            web_static_dir: None,
            acme_email: None,
            acme_domains: vec![],
            acme_dir: PathBuf::from("/data/acme"),
            acme_staging: false,
            mta_sts_mode: None,
            mta_sts_mx: vec![],
            mta_sts_max_age: 604800,
            mta_sts_id: chrono::Utc::now().format("%Y%m%d%H%M%S").to_string(),
            spam_score_threshold: 5.0,
            clamav_addr: None,
            llm_url: "https://devops.golia.jp/api/llm/complete".into(),
            llm_api_key: None,
            ai_analysis_enabled: false,
            auth_max_failures_account: 5,
            auth_account_window_secs: 900,
            auth_base_lockout_secs: 1800,
            auth_max_failures_ip: 20,
            auth_ip_window_secs: 3600,
            auth_ip_base_lockout_secs: 3600,
            auth_backoff_multiplier: 2.0,
            auth_max_lockout_secs: 86400,
            srs_secret: None,
            pg_url: None,
            valkey_url: None,
            meili_url: None,
            meili_key: None,
            chrome_cdp_url: None,
            ldap_url: None,
            ldap_bind_dn: None,
            ldap_bind_password: None,
            ldap_base_dn: None,
            ldap_user_filter: None,
            webhook_url: None,
            webhook_api_key: None,
        }
    }
}

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
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        cfg.load_network();
        cfg.load_storage();
        cfg.load_tls();
        cfg.load_acme();
        cfg.load_inbound_policy();
        cfg.load_dkim_signing();
        cfg.load_mta_sts();
        cfg.load_anti_abuse();
        cfg.load_auth_guard();
        cfg.load_external_services();
        cfg.load_ldap();
        cfg.load_webhook();
        cfg
    }

    /// Network listeners + hostname + maildir root + local domains.
    fn load_network(&mut self) {
        set_string("MAILRS_HOSTNAME", &mut self.hostname);
        set_string("MAILRS_MAILDIR", &mut self.maildir_root);
        set_parsed("MAILRS_PORT", &mut self.smtp_port);
        set_parsed("MAILRS_SUBMISSION_PORT", &mut self.submission_port);
        set_parsed("MAILRS_SMTPS_PORT", &mut self.smtps_port);
        set_parsed("MAILRS_WEB_PORT", &mut self.web_port);
        set_parsed("MAILRS_IMAP_PORT", &mut self.imap_port);
        set_parsed("MAILRS_IMAPS_PORT", &mut self.imaps_port);
        set_parsed("MAILRS_POP3_PORT", &mut self.pop3_port);
        set_parsed("MAILRS_MANAGESIEVE_PORT", &mut self.managesieve_port);
        set_csv_lower("MAILRS_LOCAL_DOMAINS", &mut self.local_domains);
        set_opt_path("MAILRS_WEB_STATIC_DIR", &mut self.web_static_dir);
        set_opt_path("MAILRS_USERS_FILE", &mut self.users_file);
    }

    /// PostgreSQL + Valkey URLs.
    fn load_storage(&mut self) {
        set_opt_string("MAILRS_PG_URL", &mut self.pg_url);
        set_opt_string("MAILRS_VALKEY_URL", &mut self.valkey_url);
    }

    /// Manual TLS cert + key paths (vs ACME auto-issued).
    fn load_tls(&mut self) {
        set_opt_path("MAILRS_TLS_CERT", &mut self.tls_cert);
        set_opt_path("MAILRS_TLS_KEY", &mut self.tls_key);
    }

    /// ACME (Let's Encrypt) automatic cert issuance.
    fn load_acme(&mut self) {
        set_opt_string_nonempty("MAILRS_ACME_EMAIL", &mut self.acme_email);
        set_csv("MAILRS_ACME_DOMAINS", &mut self.acme_domains);
        if let Ok(v) = std::env::var("MAILRS_ACME_DIR") {
            self.acme_dir = PathBuf::from(v);
        }
        set_bool_loose("MAILRS_ACME_STAGING", &mut self.acme_staging);
    }

    /// Inbound rate-limit + DNSBL + greylist + anti-spam toggle +
    /// smuggle-protection mode.
    fn load_inbound_policy(&mut self) {
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
    fn load_dkim_signing(&mut self) {
        set_opt_string_nonempty("MAILRS_DKIM_SELECTOR", &mut self.dkim_selector);
        set_opt_string_nonempty("MAILRS_DKIM_DOMAIN", &mut self.dkim_domain);
        if let Ok(v) = std::env::var("MAILRS_DKIM_PRIVATE_KEY")
            && !v.is_empty()
        {
            self.dkim_private_key_path = Some(PathBuf::from(v));
        }
    }

    /// MTA-STS publishing config (mode + MX list + max-age + id).
    fn load_mta_sts(&mut self) {
        set_opt_string("MAILRS_MTA_STS_MODE", &mut self.mta_sts_mode);
        set_csv("MAILRS_MTA_STS_MX", &mut self.mta_sts_mx);
        set_parsed("MAILRS_MTA_STS_MAX_AGE", &mut self.mta_sts_max_age);
        set_string("MAILRS_MTA_STS_ID", &mut self.mta_sts_id);
    }

    /// Spam / ClamAV / LLM / AI-analysis + SRS secret.
    fn load_anti_abuse(&mut self) {
        set_parsed("MAILRS_SPAM_SCORE_THRESHOLD", &mut self.spam_score_threshold);
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
    fn load_auth_guard(&mut self) {
        set_parsed("MAILRS_AUTH_MAX_FAILURES", &mut self.auth_max_failures_account);
        set_parsed("MAILRS_AUTH_ACCOUNT_WINDOW_SECS", &mut self.auth_account_window_secs);
        set_parsed("MAILRS_AUTH_LOCKOUT_SECS", &mut self.auth_base_lockout_secs);
        set_parsed("MAILRS_AUTH_MAX_FAILURES_IP", &mut self.auth_max_failures_ip);
        set_parsed("MAILRS_AUTH_IP_WINDOW_SECS", &mut self.auth_ip_window_secs);
        set_parsed("MAILRS_AUTH_IP_LOCKOUT_SECS", &mut self.auth_ip_base_lockout_secs);
        set_parsed("MAILRS_AUTH_BACKOFF_MULTIPLIER", &mut self.auth_backoff_multiplier);
        set_parsed("MAILRS_AUTH_MAX_LOCKOUT_SECS", &mut self.auth_max_lockout_secs);
    }

    /// Chrome CDP for HTML preview + Meilisearch full-text index.
    fn load_external_services(&mut self) {
        set_opt_string_nonempty("MAILRS_CHROME_CDP_URL", &mut self.chrome_cdp_url);
        set_opt_string_nonempty("MAILRS_MEILI_URL", &mut self.meili_url);
        set_opt_string_nonempty("MAILRS_MEILI_KEY", &mut self.meili_key);
    }

    /// LDAP fallback-auth backend.
    fn load_ldap(&mut self) {
        set_opt_string_nonempty("MAILRS_LDAP_URL", &mut self.ldap_url);
        set_opt_string_nonempty("MAILRS_LDAP_BIND_DN", &mut self.ldap_bind_dn);
        set_opt_string_nonempty("MAILRS_LDAP_BIND_PASSWORD", &mut self.ldap_bind_password);
        set_opt_string_nonempty("MAILRS_LDAP_BASE_DN", &mut self.ldap_base_dn);
        set_opt_string_nonempty("MAILRS_LDAP_USER_FILTER", &mut self.ldap_user_filter);
    }

    /// Global webhook (every email event → POST <url>).
    fn load_webhook(&mut self) {
        set_opt_string_nonempty("MAILRS_WEBHOOK_URL", &mut self.webhook_url);
        set_opt_string_nonempty("MAILRS_WEBHOOK_API_KEY", &mut self.webhook_api_key);
    }

    /// validate configuration and return warnings
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.hostname == "localhost" || self.hostname.is_empty() {
            warnings.push("MAILRS_HOSTNAME not set or is 'localhost' — mail delivery will fail".into());
        }

        if self.local_domains.is_empty() {
            warnings.push("MAILRS_LOCAL_DOMAINS is empty — no domains will accept mail".into());
        }

        if self.tls_mode() == TlsMode::None {
            warnings.push("No TLS configured — STARTTLS and IMAPS will be unavailable".into());
        }

        if self.dkim_selector.is_some() && self.dkim_private_key_path.is_none() {
            warnings.push("DKIM selector set but no private key path — DKIM signing will fail".into());
        }

        if self.mta_sts_mode.is_some() && self.mta_sts_mx.is_empty() {
            warnings.push("MTA-STS mode set but no MX hosts — policy will be invalid".into());
        }

        if let Some(ref url) = self.valkey_url
            && let Err(e) = crate::valkey_store::validate_url(url) {
                warnings.push(format!("MAILRS_VALKEY_URL is invalid: {e}"));
            }

        if self.ldap_url.is_some()
            && (self.ldap_bind_dn.is_none()
                || self.ldap_bind_password.is_none()
                || self.ldap_base_dn.is_none())
        {
            warnings.push(
                "MAILRS_LDAP_URL set but LDAP_BIND_DN, LDAP_BIND_PASSWORD, or LDAP_BASE_DN missing — LDAP auth disabled".into(),
            );
        }

        warnings
    }

    /// build an LdapConfig if all required LDAP env vars are set
    pub fn ldap_config(&self) -> Option<crate::ldap_auth::LdapConfig> {
        let url = self.ldap_url.as_ref()?;
        let bind_dn = self.ldap_bind_dn.as_ref()?;
        let bind_password = self.ldap_bind_password.as_ref()?;
        let base_dn = self.ldap_base_dn.as_ref()?;
        let user_filter = self
            .ldap_user_filter
            .clone()
            .unwrap_or_else(|| "(&(objectClass=person)(mail={}))".into());
        Some(crate::ldap_auth::LdapConfig {
            url: url.clone(),
            bind_dn: bind_dn.clone(),
            bind_password: bind_password.clone(),
            base_dn: base_dn.clone(),
            user_filter,
        })
    }

    pub fn has_tls(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }

    pub fn tls_mode(&self) -> TlsMode {
        if self.acme_email.is_some() {
            TlsMode::Acme
        } else if self.has_tls() {
            TlsMode::Manual
        } else {
            TlsMode::None
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

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

mod loaders;

#[derive(Debug, PartialEq)]
pub enum TlsMode {
    Acme,
    Manual,
    None,
}

pub struct ServerConfig {
    pub hostname: String,
    pub maildir_root: String,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub users_file: Option<PathBuf>,
    // ACME
    pub acme_email: Option<String>,
    pub acme_domains: Vec<String>,
    pub acme_dir: PathBuf,
    pub acme_staging: bool,
    // AI email analysis (self-hosted LLM)
    pub llm_url: String,
    pub llm_api_key: Option<String>,
    pub ai_analysis_enabled: bool,
    // storage backends
    pub pg_url: Option<String>,
    /// spg-embedded only: force-unlock the catalog before opening it.
    /// For single-instance deployments (docker compose) where this
    /// server is the data dir's only legitimate writer — a SIGKILLed
    /// predecessor (deploy recreate beats the 10 s docker grace) leaves
    /// a lock the namespace-honest 7.27 probe refuses to reclaim.
    pub spg_force_unlock: bool,
    /// Persistence directory for the in-process kevy embedded store.
    /// `None` keeps kevy memory-only (lost on restart). The directory is
    /// created if missing; kevy writes `aof-0.aof` + `dump-0.rdb` here.
    pub kevy_data_dir: Option<std::path::PathBuf>,
    /// URL of the shared network kevy-server (e.g. `kevy://host:6379`),
    /// which carries the cross-process event channel.
    pub kevy_url: Option<String>,
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
            tls_cert: None,
            tls_key: None,
            users_file: None,
            acme_email: None,
            acme_domains: vec![],
            acme_dir: PathBuf::from("/data/acme"),
            acme_staging: false,
            llm_url: String::new(),
            llm_api_key: None,
            ai_analysis_enabled: false,
            pg_url: None,
            spg_force_unlock: false,
            kevy_data_dir: None,
            kevy_url: None,
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

impl ServerConfig {
    /// Load configuration from `MAILRS_*` environment variables,
    /// falling back to [`ServerConfig::default`] for anything unset.
    /// The actual per-section loaders live in `config/loaders.rs`.
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        cfg.load_network();
        cfg.load_storage();
        cfg.load_tls();
        cfg.load_acme();
        cfg.load_ai();
        cfg.load_ldap();
        cfg.load_webhook();
        cfg
    }

    /// validate configuration and return warnings
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.hostname == "localhost" || self.hostname.is_empty() {
            warnings
                .push("MAILRS_HOSTNAME not set or is 'localhost' — mail delivery will fail".into());
        }

        if self.tls_mode() == TlsMode::None {
            warnings.push("No TLS configured — STARTTLS and IMAPS will be unavailable".into());
        }

        if let Some(ref dir) = self.kevy_data_dir
            && dir.exists()
            && !dir.is_dir()
        {
            warnings.push(format!(
                "MAILRS_KEVY_DATA_DIR points to a non-directory: {}",
                dir.display()
            ));
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
        // Manual cert wins over ACME: if both MAILRS_TLS_CERT/KEY and
        // MAILRS_ACME_EMAIL are set, the operator is telling us they have
        // a real cert; treat ACME as a fallback for when no manual cert
        // exists. v1.7.99 incident: ACME init panicked on LE rate-limit
        // while a perfectly good manual cert was sitting next to it.
        if self.has_tls() {
            TlsMode::Manual
        } else if self.acme_email.is_some() {
            TlsMode::Acme
        } else {
            TlsMode::None
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn tls_mode_manual_takes_precedence_over_acme() {
        // v1.7.99 incident — both manual cert and acme email were set;
        // the old priority chose ACME, which panicked on LE rate-limit
        // and crashed startup while the manual cert was perfectly valid.
        let cfg = ServerConfig {
            tls_cert: Some(PathBuf::from("/etc/ssl/cert.pem")),
            tls_key: Some(PathBuf::from("/etc/ssl/key.pem")),
            acme_email: Some("admin@example.com".into()),
            ..ServerConfig::default()
        };
        assert_eq!(cfg.tls_mode(), TlsMode::Manual);
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
    // validate — TLS warnings
    // =====================================================================

    #[test]
    fn validate_warns_on_no_tls() {
        let cfg = ServerConfig::default();
        let warnings = cfg.validate();
        assert!(warnings.iter().any(|w| w.contains("No TLS configured")));
    }

    // =====================================================================
    // default values — comprehensive
    // =====================================================================

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
    fn default_ai_is_disabled() {
        let cfg = ServerConfig::default();
        assert!(!cfg.ai_analysis_enabled);
        assert_eq!(cfg.llm_url, "");
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
        assert_eq!(
            cfg.users_file,
            Some(PathBuf::from("/etc/mailrs/users.toml"))
        );
        clear_mailrs_env();
    }

    // =====================================================================
    // from_env — comma-separated lists
    // =====================================================================

    #[test]
    fn from_env_acme_domains() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_mailrs_env();
        unsafe { std::env::set_var("MAILRS_ACME_DOMAINS", "mx.example.com, mail.example.com") };
        let cfg = ServerConfig::from_env();
        assert_eq!(cfg.acme_domains, vec!["mx.example.com", "mail.example.com"]);
        clear_mailrs_env();
    }

    // =====================================================================
    // from_env — boolean parsing
    // =====================================================================

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

    // =====================================================================
    // from_env — storage backends
    // =====================================================================

    #[test]
    fn from_env_pg_url() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_mailrs_env();
        unsafe { std::env::set_var("MAILRS_PG_URL", "postgres://user:pass@localhost/mailrs") };
        let cfg = ServerConfig::from_env();
        assert_eq!(
            cfg.pg_url,
            Some("postgres://user:pass@localhost/mailrs".into())
        );
        clear_mailrs_env();
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
}

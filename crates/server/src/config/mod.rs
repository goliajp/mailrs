//! Server configuration loaded from `MAILRS_*` environment variables.
//!
//! The loaders read through [`ServerConfig::from_vars`], which the tests
//! call with a table of their own: a test that set and cleared `MAILRS_*`
//! in the process environment removed variables other tests in the same
//! binary, running in parallel, had just set.

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
        Self::from_vars(&|name| std::env::var(name).ok())
    }

    /// The same, from `env` instead of the process environment.
    pub(crate) fn from_vars(env: loaders::Env<'_>) -> Self {
        let mut cfg = Self::default();
        cfg.load_network(env);
        cfg.load_storage(env);
        cfg.load_tls(env);
        cfg.load_acme(env);
        cfg.load_ai(env);
        cfg.load_ldap(env);
        cfg.load_webhook(env);
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

    /// The configuration these variables, and no others, give. Never the
    /// process environment: tests in this binary run in parallel and some
    /// of them set variables of their own.
    fn config_of(vars: &[(&str, &str)]) -> ServerConfig {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ServerConfig::from_vars(&move |name| {
            vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
        })
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
        let cfg = config_of(&[("MAILRS_HOSTNAME", "mail.test.io")]);
        assert_eq!(cfg.hostname, "mail.test.io");
    }

    #[test]
    fn from_env_maildir() {
        let cfg = config_of(&[("MAILRS_MAILDIR", "/var/mail")]);
        assert_eq!(cfg.maildir_root, "/var/mail");
    }

    // =====================================================================
    // from_env — TLS cert/key paths
    // =====================================================================

    #[test]
    fn from_env_tls_cert_and_key() {
        let cfg = config_of(&[
            ("MAILRS_TLS_CERT", "/etc/ssl/cert.pem"),
            ("MAILRS_TLS_KEY", "/etc/ssl/key.pem"),
        ]);
        assert_eq!(cfg.tls_cert, Some(PathBuf::from("/etc/ssl/cert.pem")));
        assert_eq!(cfg.tls_key, Some(PathBuf::from("/etc/ssl/key.pem")));
    }

    #[test]
    fn from_env_users_file() {
        let cfg = config_of(&[("MAILRS_USERS_FILE", "/etc/mailrs/users.toml")]);
        assert_eq!(
            cfg.users_file,
            Some(PathBuf::from("/etc/mailrs/users.toml"))
        );
    }

    // =====================================================================
    // from_env — comma-separated lists
    // =====================================================================

    #[test]
    fn from_env_acme_domains() {
        let cfg = config_of(&[("MAILRS_ACME_DOMAINS", "mx.example.com, mail.example.com")]);
        assert_eq!(cfg.acme_domains, vec!["mx.example.com", "mail.example.com"]);
    }

    // =====================================================================
    // from_env — boolean parsing
    // =====================================================================

    #[test]
    fn from_env_acme_staging_true() {
        let cfg = config_of(&[("MAILRS_ACME_STAGING", "true")]);
        assert!(cfg.acme_staging);
    }

    #[test]
    fn from_env_acme_staging_one() {
        let cfg = config_of(&[("MAILRS_ACME_STAGING", "1")]);
        assert!(cfg.acme_staging);
    }

    #[test]
    fn from_env_acme_staging_false_for_other_values() {
        let cfg = config_of(&[("MAILRS_ACME_STAGING", "yes")]);
        // only "1" and "true" (case-insensitive) enable it
        assert!(!cfg.acme_staging);
    }

    #[test]
    fn from_env_ai_analysis_enabled() {
        let cfg = config_of(&[("MAILRS_AI_ANALYSIS_ENABLED", "true")]);
        assert!(cfg.ai_analysis_enabled);
    }

    #[test]
    fn from_env_ai_analysis_enabled_one() {
        let cfg = config_of(&[("MAILRS_AI_ANALYSIS_ENABLED", "1")]);
        assert!(cfg.ai_analysis_enabled);
    }

    // =====================================================================
    // from_env — ACME fields
    // =====================================================================

    #[test]
    fn from_env_acme_email_non_empty() {
        let cfg = config_of(&[("MAILRS_ACME_EMAIL", "admin@example.com")]);
        assert_eq!(cfg.acme_email, Some("admin@example.com".into()));
    }

    #[test]
    fn from_env_acme_email_empty_stays_none() {
        let cfg = config_of(&[("MAILRS_ACME_EMAIL", "")]);
        assert_eq!(cfg.acme_email, None);
    }

    #[test]
    fn from_env_acme_dir() {
        let cfg = config_of(&[("MAILRS_ACME_DIR", "/custom/acme")]);
        assert_eq!(cfg.acme_dir, PathBuf::from("/custom/acme"));
    }

    // =====================================================================
    // from_env — LLM / ClamAV
    // =====================================================================

    #[test]
    fn from_env_llm_url() {
        let cfg = config_of(&[("MAILRS_LLM_URL", "https://custom.llm/api")]);
        assert_eq!(cfg.llm_url, "https://custom.llm/api");
    }

    // =====================================================================
    // from_env — storage backends
    // =====================================================================

    #[test]
    fn from_env_pg_url() {
        let cfg = config_of(&[("MAILRS_PG_URL", "postgres://user:pass@localhost/mailrs")]);
        assert_eq!(
            cfg.pg_url,
            Some("postgres://user:pass@localhost/mailrs".into())
        );
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

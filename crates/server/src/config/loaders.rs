//! Env-var → field loaders for [`super::ServerConfig`].
//!
//! Each `load_*` method maps one logical section of
//! `MAILRS_*` environment variables onto the corresponding struct
//! fields. They are called in fixed order by `ServerConfig::from_env`
//! so subsequent loaders can rely on previously-set values if needed.

use std::path::PathBuf;

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
    /// Hostname, maildir root, the users file.
    pub(super) fn load_network(&mut self) {
        set_string("MAILRS_HOSTNAME", &mut self.hostname);
        set_string("MAILRS_MAILDIR", &mut self.maildir_root);
        set_opt_path("MAILRS_USERS_FILE", &mut self.users_file);
    }

    /// PostgreSQL URL + kevy embedded persistence directory.
    pub(super) fn load_storage(&mut self) {
        set_opt_string("MAILRS_PG_URL", &mut self.pg_url);
        set_bool_truthy("MAILRS_SPG_FORCE_UNLOCK", &mut self.spg_force_unlock);
        set_opt_path("MAILRS_KEVY_DATA_DIR", &mut self.kevy_data_dir);
        // the shared network kevy-server: side state and the cross-process
        // event channel
        set_opt_string_nonempty("MAILRS_KEVY_URL", &mut self.kevy_url);
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

    /// The self-hosted LLM behind the background analyzer.
    pub(super) fn load_ai(&mut self) {
        set_string("MAILRS_LLM_URL", &mut self.llm_url);
        set_opt_string_nonempty("MAILRS_LLM_API_KEY", &mut self.llm_api_key);
        if std::env::var("MAILRS_AI_ANALYSIS_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
        {
            self.ai_analysis_enabled = true;
        }
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

//! Receiver-process configuration (P6).
//!
//! A deliberately small env-driven config — the standalone receiver only
//! needs SMTP I/O, antispam, anti (via kevy-server), and spool settings. It
//! does NOT load the server's spg / web / imap / pop3 config: the receiver
//! resolves nothing and stores nothing in spg. Kept separate from the
//! server's `ServerConfig` so the receiver crate stays free of the server
//! crate.

use std::path::PathBuf;

use mailrs_smtp_codec::SmuggleProtection;

/// Everything the receiver binary reads from the environment.
#[derive(Debug, Clone)]
pub struct ReceiverConfig {
    pub hostname: String,
    /// spool maildir root; the receiver writes to `{spool_root}/incoming`.
    pub spool_root: String,
    /// shared network kevy-server URL (`kevy://host:port`) — REQUIRED in the
    /// split topology: anti state + the SpoolDelivered notify go over it.
    pub kevy_url: String,
    pub smtp_port: u16,
    pub submission_port: u16,
    pub smtps_port: u16,
    pub local_domains: Vec<String>,
    pub dnsbl_zones: Vec<String>,
    pub dnsbl_enabled: bool,
    pub antispam_enabled: bool,
    pub smuggle_protection: SmuggleProtection,
    pub clamav_addr: Option<String>,
    pub greylist_delay_secs: u64,
    pub rate_limit_capacity: u32,
    pub rate_limit_refill: f64,
    pub spam_score_threshold: f64,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub users_file: Option<PathBuf>,
    pub srs_secret: Option<String>,
}

fn env_str(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn env_parsed<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_csv(name: &str, lower: bool) -> Vec<String> {
    std::env::var(name)
        .map(|v| {
            v.split(',')
                .map(|s| {
                    let t = s.trim();
                    if lower {
                        t.to_lowercase()
                    } else {
                        t.to_string()
                    }
                })
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn env_opt_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn env_bool_truthy(name: &str, default: bool) -> bool {
    std::env::var(name)
        .map(|v| v != "0" && v.to_lowercase() != "false")
        .unwrap_or(default)
}

impl ReceiverConfig {
    pub fn from_env() -> Self {
        let maildir_root = env_str("MAILRS_MAILDIR", "/tmp/mailrs");
        let spool_root = match env_opt_nonempty("MAILRS_SPOOL_ROOT") {
            Some(s) => s,
            None => format!("{maildir_root}/.spool"),
        };
        let smuggle_protection = match env_str("MAILRS_SMUGGLE_PROTECTION", "permissive")
            .to_lowercase()
            .as_str()
        {
            "strict" => SmuggleProtection::Strict,
            "off" => SmuggleProtection::Off,
            _ => SmuggleProtection::Permissive,
        };
        Self {
            hostname: env_str("MAILRS_HOSTNAME", "mx.mailrs.local"),
            spool_root,
            kevy_url: env_str("MAILRS_KEVY_URL", ""),
            smtp_port: env_parsed("MAILRS_PORT", 2525),
            submission_port: env_parsed("MAILRS_SUBMISSION_PORT", 2587),
            smtps_port: env_parsed("MAILRS_SMTPS_PORT", 2465),
            local_domains: env_csv("MAILRS_LOCAL_DOMAINS", true),
            dnsbl_zones: env_csv("MAILRS_DNSBL_ZONES", false),
            dnsbl_enabled: env_bool_truthy("MAILRS_DNSBL_ENABLED", false),
            antispam_enabled: env_bool_truthy("MAILRS_ANTISPAM_ENABLED", true),
            smuggle_protection,
            clamav_addr: env_opt_nonempty("MAILRS_CLAMAV_ADDR"),
            greylist_delay_secs: env_parsed("MAILRS_GREYLIST_DELAY", 60),
            rate_limit_capacity: env_parsed("MAILRS_RATE_LIMIT_CAPACITY", 100),
            rate_limit_refill: env_parsed("MAILRS_RATE_LIMIT_REFILL", 1.0),
            spam_score_threshold: env_parsed("MAILRS_SPAM_SCORE_THRESHOLD", 5.0),
            tls_cert: env_opt_nonempty("MAILRS_TLS_CERT").map(PathBuf::from),
            tls_key: env_opt_nonempty("MAILRS_TLS_KEY").map(PathBuf::from),
            users_file: env_opt_nonempty("MAILRS_USERS_FILE").map(PathBuf::from),
            srs_secret: env_opt_nonempty("MAILRS_SRS_SECRET"),
        }
    }
}

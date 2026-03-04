use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub enum TlsMode {
    Acme,
    Manual,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SmuggleProtection {
    Strict,
    Permissive,
    Off,
}

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
    // AI spam classification
    pub ai_enabled: bool,
    pub ai_api_url: String,
    pub ai_api_key: Option<String>,
    pub ai_model: String,
    // ClamAV
    pub clamav_addr: Option<String>,
    // AI email analysis (Gemini)
    pub gemini_api_key: Option<String>,
    pub ai_analysis_enabled: bool,
    // storage backends
    pub pg_url: Option<String>,
    pub valkey_url: Option<String>,
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
            ai_enabled: false,
            ai_api_url: "https://api.anthropic.com/v1/messages".into(),
            ai_api_key: None,
            ai_model: "claude-haiku-4-5-20251001".into(),
            clamav_addr: None,
            gemini_api_key: None,
            ai_analysis_enabled: false,
            pg_url: None,
            valkey_url: None,
        }
    }
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();

        if let Ok(v) = std::env::var("MAILRS_HOSTNAME") {
            cfg.hostname = v;
        }
        if let Ok(v) = std::env::var("MAILRS_MAILDIR") {
            cfg.maildir_root = v;
        }
        if let Ok(v) = std::env::var("MAILRS_PORT") {
            if let Ok(p) = v.parse() {
                cfg.smtp_port = p;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_SUBMISSION_PORT") {
            if let Ok(p) = v.parse() {
                cfg.submission_port = p;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_SMTPS_PORT") {
            if let Ok(p) = v.parse() {
                cfg.smtps_port = p;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_TLS_CERT") {
            cfg.tls_cert = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("MAILRS_TLS_KEY") {
            cfg.tls_key = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("MAILRS_USERS_FILE") {
            cfg.users_file = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("MAILRS_WEB_PORT") {
            if let Ok(p) = v.parse() {
                cfg.web_port = p;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_IMAP_PORT") {
            if let Ok(p) = v.parse() {
                cfg.imap_port = p;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_IMAPS_PORT") {
            if let Ok(p) = v.parse() {
                cfg.imaps_port = p;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_LOCAL_DOMAINS") {
            cfg.local_domains = v.split(',').map(|s| s.trim().to_lowercase()).collect();
        }
        if let Ok(v) = std::env::var("MAILRS_DNSBL_ZONES") {
            cfg.dnsbl_zones = v.split(',').map(|s| s.trim().to_string()).collect();
        }
        if let Ok(v) = std::env::var("MAILRS_RATE_LIMIT_CAPACITY") {
            if let Ok(n) = v.parse() {
                cfg.rate_limit_capacity = n;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_RATE_LIMIT_REFILL") {
            if let Ok(n) = v.parse() {
                cfg.rate_limit_refill = n;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_GREYLIST_DELAY") {
            if let Ok(n) = v.parse() {
                cfg.greylist_delay_secs = n;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_DNSBL_ENABLED") {
            cfg.dnsbl_enabled = v != "0" && v.to_lowercase() != "false";
        }
        if let Ok(v) = std::env::var("MAILRS_ANTISPAM_ENABLED") {
            cfg.antispam_enabled = v != "0" && v.to_lowercase() != "false";
        } else if let Ok(v) = std::env::var("MAILRS_SPF_ENABLED") {
            // backwards compatibility
            cfg.antispam_enabled = v != "0" && v.to_lowercase() != "false";
        }
        if let Ok(v) = std::env::var("MAILRS_SMUGGLE_PROTECTION") {
            cfg.smuggle_protection = match v.to_lowercase().as_str() {
                "strict" => SmuggleProtection::Strict,
                "off" => SmuggleProtection::Off,
                _ => SmuggleProtection::Permissive,
            };
        }
        if let Ok(v) = std::env::var("MAILRS_DKIM_SELECTOR") {
            if !v.is_empty() {
                cfg.dkim_selector = Some(v);
            }
        }
        if let Ok(v) = std::env::var("MAILRS_DKIM_DOMAIN") {
            if !v.is_empty() {
                cfg.dkim_domain = Some(v);
            }
        }
        if let Ok(v) = std::env::var("MAILRS_DKIM_PRIVATE_KEY") {
            if !v.is_empty() {
                cfg.dkim_private_key_path = Some(PathBuf::from(v));
            }
        }
        if let Ok(v) = std::env::var("MAILRS_WEB_STATIC_DIR") {
            cfg.web_static_dir = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("MAILRS_ACME_EMAIL") {
            if !v.is_empty() {
                cfg.acme_email = Some(v);
            }
        }
        if let Ok(v) = std::env::var("MAILRS_ACME_DOMAINS") {
            cfg.acme_domains = v.split(',').map(|s| s.trim().to_string()).collect();
        }
        if let Ok(v) = std::env::var("MAILRS_ACME_DIR") {
            cfg.acme_dir = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("MAILRS_ACME_STAGING") {
            cfg.acme_staging = v == "1" || v.to_lowercase() == "true";
        }
        if let Ok(v) = std::env::var("MAILRS_MTA_STS_MODE") {
            cfg.mta_sts_mode = Some(v);
        }
        if let Ok(v) = std::env::var("MAILRS_MTA_STS_MX") {
            cfg.mta_sts_mx = v.split(',').map(|s| s.trim().to_string()).collect();
        }
        if let Ok(v) = std::env::var("MAILRS_MTA_STS_MAX_AGE") {
            if let Ok(n) = v.parse() {
                cfg.mta_sts_max_age = n;
            }
        }
        if let Ok(v) = std::env::var("MAILRS_MTA_STS_ID") {
            cfg.mta_sts_id = v;
        }
        if let Ok(v) = std::env::var("MAILRS_SPAM_SCORE_THRESHOLD") {
            if let Ok(n) = v.parse() {
                cfg.spam_score_threshold = n;
            }
        }
        if std::env::var("MAILRS_AI_ENABLED").map(|v| v == "true" || v == "1").unwrap_or(false) {
            cfg.ai_enabled = true;
        }
        if let Ok(v) = std::env::var("MAILRS_AI_API_URL") {
            cfg.ai_api_url = v;
        }
        if let Ok(v) = std::env::var("MAILRS_AI_API_KEY") {
            cfg.ai_api_key = Some(v);
        }
        if let Ok(v) = std::env::var("MAILRS_AI_MODEL") {
            cfg.ai_model = v;
        }
        if let Ok(v) = std::env::var("MAILRS_CLAMAV_ADDR") {
            cfg.clamav_addr = Some(v);
        }
        if let Ok(v) = std::env::var("MAILRS_GEMINI_API_KEY") {
            cfg.gemini_api_key = Some(v);
        }
        if std::env::var("MAILRS_AI_ANALYSIS_ENABLED").map(|v| v == "true" || v == "1").unwrap_or(false) {
            cfg.ai_analysis_enabled = true;
        }
        if let Ok(v) = std::env::var("MAILRS_PG_URL") {
            cfg.pg_url = Some(v);
        }
        if let Ok(v) = std::env::var("MAILRS_VALKEY_URL") {
            cfg.valkey_url = Some(v);
        }

        cfg
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

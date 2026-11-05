//! `MAILRS_OPS_*`: everything the daemon is told by the deployment.

use std::path::PathBuf;

/// The external SMTP relay channel, as configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayConfig {
    pub host: String,
    pub port: u16,
    pub starttls: bool,
    pub user: Option<String>,
    pub password: Option<String>,
    pub from: String,
    pub to: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: String,
    pub data_dir: PathBuf,
    pub interval_secs: i64,
    pub group_secs: i64,
    pub unknown_wait_secs: i64,
    pub fastcore_url: String,
    pub webapi_url: String,
    pub ops_token: Option<String>,
    pub master_key: Option<String>,
    pub admin_secret_sha256: Option<String>,
    pub heartbeat_url: Option<String>,
    pub admin_url: Option<String>,
    pub max_per_hour: u32,
    pub push_minimal: bool,
    pub webhook_url: Option<String>,
    pub webhook_secret: Option<String>,
    pub push_url: Option<String>,
    pub push_token: Option<String>,
    pub relay: Option<RelayConfig>,
    pub local_domains: Vec<String>,
    pub maintenance_alert_secs: i64,
    pub push_sources: Vec<String>,
    pub new_domains_per_day: u64,
    pub fastcore_ready_secs: i64,
    pub hostname: String,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn number<T: std::str::FromStr>(name: &str, default: T) -> Result<T, String> {
    match var(name) {
        None => Ok(default),
        Some(v) => v
            .trim()
            .parse()
            .map_err(|_| format!("{name}={v:?}: expected a number")),
    }
}

fn list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let relay = match var("MAILRS_OPS_RELAY_HOST") {
            None => None,
            Some(host) => Some(RelayConfig {
                host,
                port: number("MAILRS_OPS_RELAY_PORT", 587)?,
                starttls: match var("MAILRS_OPS_RELAY_TLS").as_deref() {
                    None | Some("starttls") => true,
                    Some("none") => false,
                    Some(other) => {
                        return Err(format!(
                            "MAILRS_OPS_RELAY_TLS={other:?}: expected starttls or none"
                        ));
                    }
                },
                user: var("MAILRS_OPS_RELAY_USER"),
                password: var("MAILRS_OPS_RELAY_PASSWORD"),
                from: var("MAILRS_OPS_RELAY_FROM")
                    .ok_or("MAILRS_OPS_RELAY_FROM is required with MAILRS_OPS_RELAY_HOST")?,
                to: list(&var("MAILRS_OPS_RELAY_TO").unwrap_or_default()),
            }),
        };
        let push_minimal = match var("MAILRS_OPS_PUSH_MINIMAL").as_deref() {
            None | Some("1") | Some("true") | Some("on") => true,
            Some("0") | Some("false") | Some("off") => false,
            Some(other) => {
                return Err(format!(
                    "MAILRS_OPS_PUSH_MINIMAL={other:?}: expected on or off"
                ));
            }
        };
        let interval_secs: i64 = number("MAILRS_OPS_INTERVAL_SECS", 60)?;
        if interval_secs < 1 {
            return Err("MAILRS_OPS_INTERVAL_SECS must be at least 1".into());
        }
        let hours: i64 = number("MAILRS_OPS_MAINTENANCE_ALERT_HOURS", 4)?;
        Ok(Self {
            bind: var("MAILRS_OPS_BIND").unwrap_or_else(|| "0.0.0.0:3600".into()),
            data_dir: PathBuf::from(var("MAILRS_OPS_DATA_DIR").unwrap_or_else(|| "/data".into())),
            interval_secs,
            group_secs: number("MAILRS_OPS_GROUP_SECS", 30)?,
            unknown_wait_secs: number("MAILRS_OPS_UNKNOWN_WAIT_SECS", 600)?,
            fastcore_url: var("MAILRS_OPS_FASTCORE_URL")
                .unwrap_or_else(|| "http://fastcore:3301".into()),
            webapi_url: var("MAILRS_OPS_WEBAPI_URL")
                .unwrap_or_else(|| "http://webapi-fc:3100".into()),
            ops_token: var("MAILRS_OPS_TOKEN"),
            master_key: var("MAILRS_OPS_REPORT_MASTER_KEY"),
            admin_secret_sha256: var("MAILRS_OPS_ADMIN_SECRET_SHA256")
                .map(|h| h.trim().to_ascii_lowercase()),
            heartbeat_url: var("MAILRS_OPS_HEARTBEAT_URL"),
            admin_url: var("MAILRS_OPS_ADMIN_URL"),
            max_per_hour: number("MAILRS_OPS_CHANNEL_MAX_PER_HOUR", 10)?,
            push_minimal,
            webhook_url: var("MAILRS_OPS_WEBHOOK_URL"),
            webhook_secret: var("MAILRS_OPS_WEBHOOK_SECRET"),
            push_url: var("MAILRS_OPS_PUSH_URL"),
            push_token: var("MAILRS_OPS_PUSH_TOKEN"),
            relay,
            local_domains: list(&var("MAILRS_LOCAL_DOMAINS").unwrap_or_default()),
            maintenance_alert_secs: hours * 3600,
            push_sources: list(
                &var("MAILRS_OPS_PUSH_SOURCES")
                    .unwrap_or_else(|| "receiver,sender,fetch,backup".into()),
            ),
            new_domains_per_day: number("MAILRS_OPS_NEW_DOMAINS_PER_DAY", 5000)?,
            fastcore_ready_secs: number("MAILRS_OPS_FASTCORE_READY_SECS", 1800)?,
            hostname: var("MAILRS_HOSTNAME").unwrap_or_else(|| "mailrs".into()),
        })
    }

    pub fn store_path(&self) -> PathBuf {
        self.data_dir.join("ops.sqlite")
    }

    pub fn socket_path(&self) -> PathBuf {
        self.data_dir.join("ops.sock")
    }

    /// A configuration for tests: everything local, nothing sent.
    pub fn for_tests(data_dir: PathBuf) -> Self {
        Self {
            bind: "127.0.0.1:0".into(),
            data_dir,
            interval_secs: 60,
            group_secs: 30,
            unknown_wait_secs: 600,
            fastcore_url: "http://127.0.0.1:9".into(),
            webapi_url: "http://127.0.0.1:9".into(),
            ops_token: None,
            master_key: Some("test-master".into()),
            admin_secret_sha256: None,
            heartbeat_url: None,
            admin_url: Some("https://admin.example.test/ops".into()),
            max_per_hour: 10,
            push_minimal: true,
            webhook_url: None,
            webhook_secret: None,
            push_url: None,
            push_token: None,
            relay: None,
            local_domains: vec!["example.test".into()],
            maintenance_alert_secs: 4 * 3600,
            push_sources: list("receiver,sender,fetch,backup"),
            new_domains_per_day: 5000,
            fastcore_ready_secs: 1800,
            hostname: "mx.example.test".into(),
        }
    }
}

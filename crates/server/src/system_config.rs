use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::watch;

use crate::config::SmuggleProtection;

// -- RuntimeConfig: the subset of ServerConfig that can change at runtime --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    // webhook
    pub webhook_url: Option<String>,
    pub webhook_api_key: Option<String>,
    // ai
    pub ai_analysis_enabled: bool,
    pub llm_url: String,
    pub llm_api_key: Option<String>,
    // antispam
    pub antispam_enabled: bool,
    pub spam_score_threshold: f64,
    // security
    pub smuggle_protection: String, // "strict" | "permissive" | "off"
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            webhook_url: None,
            webhook_api_key: None,
            ai_analysis_enabled: false,
            llm_url: "https://devops.golia.jp/api/llm/complete".into(),
            llm_api_key: None,
            antispam_enabled: true,
            spam_score_threshold: 5.0,
            smuggle_protection: "permissive".into(),
        }
    }
}

impl RuntimeConfig {
    /// build from ServerConfig (env var snapshot at startup)
    pub fn from_server_config(cfg: &crate::config::ServerConfig) -> Self {
        Self {
            webhook_url: cfg.webhook_url.clone(),
            webhook_api_key: cfg.webhook_api_key.clone(),
            ai_analysis_enabled: cfg.ai_analysis_enabled,
            llm_url: cfg.llm_url.clone(),
            llm_api_key: cfg.llm_api_key.clone(),
            antispam_enabled: cfg.antispam_enabled,
            spam_score_threshold: cfg.spam_score_threshold,
            smuggle_protection: match cfg.smuggle_protection {
                SmuggleProtection::Strict => "strict".into(),
                SmuggleProtection::Off => "off".into(),
                SmuggleProtection::Permissive => "permissive".into(),
            },
        }
    }

    #[allow(dead_code)] // used when smtp_session reads smuggle_protection from store
    pub fn smuggle_protection_enum(&self) -> SmuggleProtection {
        match self.smuggle_protection.as_str() {
            "strict" => SmuggleProtection::Strict,
            "off" => SmuggleProtection::Off,
            _ => SmuggleProtection::Permissive,
        }
    }
}

// -- config key registry --

#[derive(Debug, Clone, Serialize)]
pub struct ConfigKeyInfo {
    pub key: &'static str,
    pub value_type: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    pub sensitive: bool,
}

pub const CONFIG_KEYS: &[ConfigKeyInfo] = &[
    ConfigKeyInfo { key: "webhook_url", value_type: "string", group: "webhook", description: "Global webhook URL for new mail notifications", sensitive: false },
    ConfigKeyInfo { key: "webhook_api_key", value_type: "string", group: "webhook", description: "Bearer token sent with webhook requests", sensitive: true },
    ConfigKeyInfo { key: "ai_analysis_enabled", value_type: "bool", group: "ai", description: "Enable AI email analysis", sensitive: false },
    ConfigKeyInfo { key: "llm_url", value_type: "string", group: "ai", description: "LLM API endpoint URL", sensitive: false },
    ConfigKeyInfo { key: "llm_api_key", value_type: "string", group: "ai", description: "LLM API authentication key", sensitive: true },
    ConfigKeyInfo { key: "antispam_enabled", value_type: "bool", group: "antispam", description: "Enable SPF/DKIM/DMARC checks", sensitive: false },
    ConfigKeyInfo { key: "spam_score_threshold", value_type: "f64", group: "antispam", description: "Spam score threshold for rejection", sensitive: false },
    ConfigKeyInfo { key: "smuggle_protection", value_type: "enum:strict,permissive,off", group: "security", description: "SMTP smuggling protection level", sensitive: false },
];

pub fn find_key(key: &str) -> Option<&'static ConfigKeyInfo> {
    CONFIG_KEYS.iter().find(|k| k.key == key)
}

fn validate_value(info: &ConfigKeyInfo, value: &str) -> Result<(), String> {
    match info.value_type {
        "bool"
            if value != "true" && value != "false" => {
                return Err(format!("expected 'true' or 'false', got '{value}'"));
            }
        "f64" => {
            value.parse::<f64>().map_err(|_| format!("expected number, got '{value}'"))?;
        }
        vt if vt.starts_with("enum:") => {
            let variants: Vec<&str> = vt[5..].split(',').collect();
            if !variants.contains(&value) {
                return Err(format!("expected one of {variants:?}, got '{value}'"));
            }
        }
        _ => {} // string: any value
    }
    Ok(())
}

// -- config entry returned by API --

#[derive(Debug, Clone, Serialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub value_type: String,
    pub group: String,
    pub description: String,
    pub source: String, // "database", "env", "default"
    pub updated_at: Option<String>,
    pub updated_by: Option<String>,
}

// -- DB row --

#[derive(Debug, sqlx::FromRow)]
struct ConfigRow {
    key: String,
    value: String,
    #[allow(dead_code)]
    value_type: String,
    updated_at: chrono::DateTime<chrono::Utc>,
    updated_by: String,
}

// -- SystemConfigStore --

const VALKEY_KEY: &str = "syscfg:all";
const RELOAD_INTERVAL: Duration = Duration::from_secs(60);

pub struct SystemConfigStore {
    pg: Option<PgPool>,
    valkey: Option<redis::aio::ConnectionManager>,
    current: ArcSwap<RuntimeConfig>,
    env_defaults: RuntimeConfig,
    /// db rows cache for metadata (updated_at, updated_by, which keys are in db)
    db_keys: std::sync::RwLock<std::collections::HashMap<String, (String, String)>>,
    // (updated_at, updated_by)
}

impl SystemConfigStore {
    pub fn new(
        pg: Option<PgPool>,
        valkey: Option<redis::aio::ConnectionManager>,
        env_defaults: RuntimeConfig,
    ) -> Self {
        Self {
            pg,
            valkey,
            current: ArcSwap::new(Arc::new(env_defaults.clone())),
            env_defaults,
            db_keys: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// zero-cost read of current config
    pub fn get(&self) -> arc_swap::Guard<Arc<RuntimeConfig>> {
        self.current.load()
    }

    /// load all config from DB, merge with env defaults, swap
    pub async fn load_from_db(&self) -> Result<(), sqlx::Error> {
        let Some(ref pool) = self.pg else {
            return Ok(());
        };

        let rows: Vec<ConfigRow> = match sqlx::query_as(
            "SELECT key, value, value_type, updated_at, updated_by FROM system_config",
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("failed to load system_config from DB: {e}");
                return Err(e);
            }
        };

        let mut merged = self.env_defaults.clone();
        let mut db_keys = std::collections::HashMap::new();

        for row in &rows {
            db_keys.insert(
                row.key.clone(),
                (row.updated_at.to_rfc3339(), row.updated_by.clone()),
            );
            apply_value(&mut merged, &row.key, &row.value);
        }

        // update db_keys metadata
        if let Ok(mut guard) = self.db_keys.write() {
            *guard = db_keys;
        }

        self.current.store(Arc::new(merged));
        Ok(())
    }

    /// set a config value: validate, write PG, invalidate cache, reload
    pub async fn set(
        &self,
        key: &str,
        value: &str,
        actor: &str,
    ) -> Result<(), String> {
        let info = find_key(key).ok_or_else(|| format!("unknown config key: {key}"))?;
        validate_value(info, value)?;

        let pool = self.pg.as_ref().ok_or("database not available")?;

        sqlx::query(
            "INSERT INTO system_config (key, value, value_type, updated_at, updated_by) \
             VALUES ($1, $2, $3, now(), $4) \
             ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = now(), updated_by = $4",
        )
        .bind(key)
        .bind(value)
        .bind(info.value_type)
        .bind(actor)
        .execute(pool)
        .await
        .map_err(|e| format!("database error: {e}"))?;

        self.valkey_del(VALKEY_KEY).await;
        self.load_from_db().await.map_err(|e| format!("reload error: {e}"))?;

        Ok(())
    }

    /// delete a DB override, revert to env default
    pub async fn delete(&self, key: &str) -> Result<(), String> {
        find_key(key).ok_or_else(|| format!("unknown config key: {key}"))?;

        let pool = self.pg.as_ref().ok_or("database not available")?;

        sqlx::query("DELETE FROM system_config WHERE key = $1")
            .bind(key)
            .execute(pool)
            .await
            .map_err(|e| format!("database error: {e}"))?;

        self.valkey_del(VALKEY_KEY).await;
        self.load_from_db().await.map_err(|e| format!("reload error: {e}"))?;

        Ok(())
    }

    /// return all config entries with source info
    pub fn get_all_entries(&self) -> Vec<ConfigEntry> {
        let current = self.current.load();
        let db_keys = self.db_keys.read().ok();

        CONFIG_KEYS
            .iter()
            .map(|info| {
                let raw_value = get_value(&current, info.key);
                let default_value = get_value(&RuntimeConfig::default(), info.key);
                let env_value = get_value(&self.env_defaults, info.key);

                let (source, updated_at, updated_by) =
                    if let Some(ref db) = db_keys {
                        if let Some((at, by)) = db.get(info.key) {
                            ("database".into(), Some(at.clone()), Some(by.clone()))
                        } else if env_value != default_value {
                            ("env".into(), None, None)
                        } else {
                            ("default".into(), None, None)
                        }
                    } else if env_value != default_value {
                        ("env".into(), None, None)
                    } else {
                        ("default".into(), None, None)
                    };

                // mask sensitive values
                let display_value = if info.sensitive {
                    mask_sensitive(&raw_value)
                } else {
                    raw_value
                };

                ConfigEntry {
                    key: info.key.to_string(),
                    value: display_value,
                    value_type: info.value_type.to_string(),
                    group: info.group.to_string(),
                    description: info.description.to_string(),
                    source,
                    updated_at,
                    updated_by,
                }
            })
            .collect()
    }

    // -- valkey helpers --

    async fn valkey_del(&self, key: &str) {
        if let Some(mut conn) = self.valkey.clone() {
            let _: std::result::Result<(), _> =
                redis::cmd("DEL").arg(key).query_async(&mut conn).await;
        }
    }
}

/// background reload task
pub async fn reload_task(
    store: Arc<SystemConfigStore>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(RELOAD_INTERVAL);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(e) = store.load_from_db().await {
                    tracing::warn!("system config reload failed: {e}");
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

// -- helpers --

fn apply_value(cfg: &mut RuntimeConfig, key: &str, value: &str) {
    match key {
        "webhook_url" => cfg.webhook_url = if value.is_empty() { None } else { Some(value.into()) },
        "webhook_api_key" => cfg.webhook_api_key = if value.is_empty() { None } else { Some(value.into()) },
        "ai_analysis_enabled" => cfg.ai_analysis_enabled = value == "true",
        "llm_url" => cfg.llm_url = value.into(),
        "llm_api_key" => cfg.llm_api_key = if value.is_empty() { None } else { Some(value.into()) },
        "antispam_enabled" => cfg.antispam_enabled = value == "true",
        "spam_score_threshold" => {
            if let Ok(v) = value.parse() {
                cfg.spam_score_threshold = v;
            }
        }
        "smuggle_protection" => cfg.smuggle_protection = value.into(),
        _ => {}
    }
}

fn get_value(cfg: &RuntimeConfig, key: &str) -> String {
    match key {
        "webhook_url" => cfg.webhook_url.clone().unwrap_or_default(),
        "webhook_api_key" => cfg.webhook_api_key.clone().unwrap_or_default(),
        "ai_analysis_enabled" => cfg.ai_analysis_enabled.to_string(),
        "llm_url" => cfg.llm_url.clone(),
        "llm_api_key" => cfg.llm_api_key.clone().unwrap_or_default(),
        "antispam_enabled" => cfg.antispam_enabled.to_string(),
        "spam_score_threshold" => cfg.spam_score_threshold.to_string(),
        "smuggle_protection" => cfg.smuggle_protection.clone(),
        _ => String::new(),
    }
}

fn mask_sensitive(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    if value.len() <= 4 {
        return "****".into();
    }
    format!("{}****", &value[..4])
}

#[cfg(test)]
#[path = "system_config_tests.rs"]
mod tests;

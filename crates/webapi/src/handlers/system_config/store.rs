//! Storage for system-config overrides: the value hash and, beside it, who
//! saved each value and when.

use axum::http::StatusCode;

use crate::handlers::kevy_util::with_kevy;

/// What `set_system_config_key` stores beside the value: when it was saved
/// and by whom. A cache of the newest `system_config.set` audit fact for the
/// key, kept so the page need not scan the audit log.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct ConfigMeta {
    pub(super) updated_at: i64,
    /// `None` for writes that came in without a known actor (the MCP tool,
    /// which records its own audit row).
    #[serde(default)]
    pub(super) updated_by: Option<String>,
}

pub(super) const CONFIG_KEY: &[u8] = b"admin:system-config";
pub(super) const CONFIG_META_KEY: &[u8] = b"admin:system-config:meta";

pub(super) fn pairs(flat: Vec<Vec<u8>>) -> std::collections::HashMap<String, String> {
    flat.chunks_exact(2)
        .map(|kv| {
            (
                String::from_utf8_lossy(&kv[0]).to_string(),
                String::from_utf8_lossy(&kv[1]).to_string(),
            )
        })
        .collect()
}

pub(super) fn with_meta(entry: &mut serde_json::Value, meta: Option<&ConfigMeta>) {
    let Some(m) = meta else { return };
    entry["updated_at"] = serde_json::json!(m.updated_at);
    if let Some(by) = &m.updated_by {
        entry["updated_by"] = serde_json::json!(by);
    }
}

pub(super) fn store(key: String, value: String, actor: Option<String>) -> Result<(), StatusCode> {
    let meta = serde_json::to_vec(&ConfigMeta {
        updated_at: crate::handlers::complete::now_secs(),
        updated_by: actor,
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(CONFIG_KEY, &[(key.as_bytes(), value.as_bytes())])?;
        c.hset(CONFIG_META_KEY, &[(key.as_bytes(), meta.as_slice())])?;
        Ok(())
    })
}

pub(super) fn remove(key: String) -> Result<(), StatusCode> {
    with_kevy(move |c| {
        c.hdel(CONFIG_KEY, &[key.as_bytes()])?;
        c.hdel(CONFIG_META_KEY, &[key.as_bytes()])?;
        Ok(())
    })
}

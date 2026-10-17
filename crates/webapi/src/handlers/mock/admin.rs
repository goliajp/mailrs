//! `/api/admin/mock`: the mock store's usage and its settings, for the
//! admin page. Behind `admin_middleware` like every `/api/admin` route.
//! Namespaces themselves are managed on `/api/mock`, where an admin can
//! list (`?all=true`), read and delete anyone's.

use axum::{Extension, Json};
use mailrs_core_sidestate::families::mock::{self, Settings};
use serde::Serialize;

use super::service::{MockApiError, store};
use crate::handlers::conversations::AuthedUser;

#[derive(Debug, Serialize)]
pub struct Usage {
    pub namespaces: usize,
    pub owners: usize,
    pub bytes: i64,
    pub received: i64,
    pub rejected_unknown_ns: i64,
    pub dropped: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminOverview {
    /// the mock domain; empty when the deployment has none configured
    pub domain: &'static str,
    pub usage: Usage,
    pub settings: Settings,
}

pub async fn overview() -> Result<Json<AdminOverview>, MockApiError> {
    let (stats, all, settings) = store(|c| {
        Ok((
            mock::stats(c)?,
            mock::list_namespaces(c, None)?,
            mock::settings(c)?,
        ))
    })?;
    let owners: std::collections::BTreeSet<&str> = all.iter().map(|n| n.owner.as_str()).collect();
    Ok(Json(AdminOverview {
        domain: mock::domain(),
        usage: Usage {
            namespaces: all.len(),
            owners: owners.len(),
            bytes: stats.bytes,
            received: stats.received,
            rejected_unknown_ns: stats.rejected_unknown_ns,
            dropped: stats.dropped,
        },
        settings,
    }))
}

/// Replace the settings; every process reads them on its next use.
pub async fn save_settings(
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(body): Json<Settings>,
) -> Result<Json<Settings>, MockApiError> {
    let before = store(move |c| {
        let before = mock::settings(c)?;
        mock::save_settings(c, &body)?;
        Ok(before)
    })?;
    let detail = changed_fields(&before, &body);
    crate::handlers::audit::record(&actor, "mock.settings_update", "", &detail);
    Ok(Json(body))
}

/// `field: old → new` for each setting that differs, comma-separated.
pub fn changed_fields(before: &Settings, after: &Settings) -> String {
    let fields = |s: &Settings| match serde_json::to_value(s) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    let (old, new) = (fields(before), fields(after));
    new.iter()
        .filter(|(k, v)| old.get(*k) != Some(*v))
        .map(|(k, v)| {
            format!(
                "{k}: {} → {v}",
                old.get(k).unwrap_or(&serde_json::Value::Null)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_settings_that_changed_are_named() {
        let before = Settings::default();
        assert_eq!(changed_fields(&before, &before), "");
        let after = Settings {
            retention_days: 7,
            create: mock::CreatePolicy::AdminsOnly,
            ..before
        };
        assert_eq!(
            changed_fields(&before, &after),
            "create: \"all-users\" → \"admins-only\", retention_days: 3 → 7"
        );
    }
}

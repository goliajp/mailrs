//! `/api/admin/mock`: the mock store's usage and its settings, for the
//! admin page. Behind `admin_middleware` like every `/api/admin` route.
//! Namespaces themselves are managed on `/api/mock`, where an admin can
//! list (`?all=true`), read and delete anyone's.

use axum::Json;
use mailrs_core_sidestate::families::mock::{self, Settings};
use serde::Serialize;

use super::service::{MockApiError, store};

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
pub async fn save_settings(Json(body): Json<Settings>) -> Result<Json<Settings>, MockApiError> {
    store(move |c| mock::save_settings(c, &body))?;
    Ok(Json(body))
}

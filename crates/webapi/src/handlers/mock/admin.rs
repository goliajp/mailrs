//! `GET /api/admin/mock`: every namespace on the deployment and the store's
//! totals, for the admin page. Behind `admin_middleware` like every
//! `/api/admin` route; deleting reuses the namespace route, which already
//! lets an admin delete any namespace.

use axum::Json;
use mailrs_core_sidestate::families::mock::{
    self, MAX_NAMESPACES_PER_USER, MAX_TOTAL_BYTES, Namespace, Stats,
};
use serde::Serialize;

use super::service::{MockApiError, store};

#[derive(Debug, Serialize)]
pub struct AdminNamespace {
    #[serde(flatten)]
    pub namespace: Namespace,
    /// bytes of the namespace's message files
    pub bytes: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminOverview {
    /// the mock domain; empty when the feature is off
    pub domain: &'static str,
    pub received: i64,
    pub rejected_unknown_ns: i64,
    pub bytes: i64,
    pub max_bytes: i64,
    pub dropped: i64,
    pub max_namespaces_per_user: usize,
    pub namespaces: Vec<AdminNamespace>,
}

pub async fn overview() -> Result<Json<AdminOverview>, MockApiError> {
    let domain = mock::domain();
    if domain.is_empty() {
        return Ok(Json(AdminOverview {
            domain,
            received: 0,
            rejected_unknown_ns: 0,
            bytes: 0,
            max_bytes: MAX_TOTAL_BYTES,
            dropped: 0,
            max_namespaces_per_user: MAX_NAMESPACES_PER_USER,
            namespaces: Vec::new(),
        }));
    }
    let (stats, all): (Stats, Vec<Namespace>) =
        store(|c| Ok((mock::stats(c)?, mock::list_namespaces(c, None)?)))?;
    let namespaces = all
        .into_iter()
        .map(|ns| AdminNamespace {
            bytes: mock::namespace_bytes(mock::root(), &ns.name),
            namespace: ns,
        })
        .collect();
    Ok(Json(AdminOverview {
        domain,
        received: stats.received,
        rejected_unknown_ns: stats.rejected_unknown_ns,
        bytes: stats.bytes,
        max_bytes: MAX_TOTAL_BYTES,
        dropped: stats.dropped,
        max_namespaces_per_user: MAX_NAMESPACES_PER_USER,
        namespaces,
    }))
}

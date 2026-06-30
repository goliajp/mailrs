//! Health probe (`/v1/healthz`) and readiness probe (`/v1/readyz`).
//!
//! Both unauthenticated — internal LB / orchestrator probes only see these.
//! Same response shape: `HealthResponse` (see `types`).

pub const PATH_HEALTHZ: &str = "/v1/healthz";
pub const PATH_READYZ: &str = "/v1/readyz";
pub const PATH_METRICS: &str = "/v1/metrics";

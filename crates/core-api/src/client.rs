//! Async HTTP client for the mailrs-core-api wire surface.
//!
//! Built on `reqwest`. webapi / sender import this with the
//! `client` feature on Cargo.toml.
//!
//! Stub — wraps `reqwest::Client` + base URL + shared auth bearer.
//! Per-method calls fill in over Phase 1 (checklist 1.12).

use crate::error::{ApiResult, CoreApiError};

/// HTTP client wrapping a single `mailrs-core` target.
///
/// One instance per process. Clonable via `Arc<Client>` (cheap).
#[derive(Clone)]
pub struct Client {
    inner: reqwest::Client,
    base_url: String,
    auth_bearer: String,
}

impl Client {
    /// Build a new client.
    ///
    /// - `base_url` — `http://core:3300` (no trailing slash). For staging
    ///   set `MAILRS_CORE_RPC_BASE` env. Local dev uses `http://127.0.0.1:3300`.
    /// - `auth_bearer` — shared secret from `MAILRS_CORE_API_SECRET` env.
    pub fn new(base_url: impl Into<String>, auth_bearer: impl Into<String>) -> Self {
        let inner = reqwest::Client::builder()
            .user_agent(concat!("mailrs-core-api/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client build");
        Self {
            inner,
            base_url: base_url.into(),
            auth_bearer: auth_bearer.into(),
        }
    }

    /// Healthz probe — does NOT include auth bearer (so LB can hit it).
    pub async fn healthz(&self) -> ApiResult<crate::types::HealthResponse> {
        let url = format!("{}{}", self.base_url, crate::method::health::PATH_HEALTHZ);
        let resp = self
            .inner
            .get(&url)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("transport: {e}")))?;
        if !resp.status().is_success() {
            return Err(CoreApiError::Internal(format!(
                "healthz returned {}",
                resp.status()
            )));
        }
        resp.json::<crate::types::HealthResponse>()
            .await
            .map_err(|e| CoreApiError::Internal(format!("decode: {e}")))
    }

    /// Internal helper for authenticated GET. Stub — used by per-method
    /// wrappers (checklist 1.12).
    #[allow(dead_code)]
    pub(crate) async fn get_authed<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> ApiResult<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .inner
            .get(&url)
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("transport: {e}")))?;
        if resp.status().as_u16() == 401 {
            return Err(CoreApiError::Unauthorized);
        }
        if resp.status().as_u16() == 503 {
            return Err(CoreApiError::PoolExhausted);
        }
        if resp.status().as_u16() == 501 {
            return Err(CoreApiError::BackendUnsupported);
        }
        resp.json::<T>()
            .await
            .map_err(|e| CoreApiError::Internal(format!("decode: {e}")))
    }
}

//! Async HTTP client for the mailrs-core-api wire surface.
//!
//! Built on `reqwest`. webapi / sender import this with the `client`
//! feature on Cargo.toml. One instance per process, clonable via Arc.

use crate::error::{ApiResult, CoreApiError};
use crate::method;
use crate::types;

/// HTTP client wrapping a single `mailrs-core` target.
#[derive(Clone)]
pub struct Client {
    pub(crate) inner: reqwest::Client,
    pub(crate) base_url: String,
    pub(crate) auth_bearer: String,
}

impl Client {
    /// Build a new client.
    pub fn new(base_url: impl Into<String>, auth_bearer: impl Into<String>) -> Self {
        let inner = reqwest::Client::builder()
            .user_agent(concat!("mailrs-core-api/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client build");
        Self {
            inner,
            base_url: base_url.into(),
            auth_bearer: auth_bearer.into(),
        }
    }

    // ── plumbing ────────────────────────────────────────────────────

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub(crate) async fn map_status<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
        context: &'static str,
    ) -> ApiResult<T> {
        let status = resp.status().as_u16();
        match status {
            200..=299 => resp
                .json::<T>()
                .await
                .map_err(|e| CoreApiError::Internal(format!("{context} decode: {e}"))),
            401 => Err(CoreApiError::Unauthorized),
            403 => Err(CoreApiError::Forbidden),
            404 => Err(CoreApiError::NotFound(context.into())),
            409 => Err(CoreApiError::Conflict(context.into())),
            501 => Err(CoreApiError::BackendUnsupported),
            503 => Err(CoreApiError::PoolExhausted),
            504 => Err(CoreApiError::Timeout),
            other => Err(CoreApiError::Internal(format!(
                "{context} returned {other}"
            ))),
        }
    }

    pub(crate) async fn map_status_unit(
        resp: reqwest::Response,
        context: &'static str,
    ) -> ApiResult<()> {
        let status = resp.status().as_u16();
        match status {
            200..=299 => Ok(()),
            401 => Err(CoreApiError::Unauthorized),
            403 => Err(CoreApiError::Forbidden),
            404 => Err(CoreApiError::NotFound(context.into())),
            409 => Err(CoreApiError::Conflict(context.into())),
            501 => Err(CoreApiError::BackendUnsupported),
            503 => Err(CoreApiError::PoolExhausted),
            504 => Err(CoreApiError::Timeout),
            other => Err(CoreApiError::Internal(format!(
                "{context} returned {other}"
            ))),
        }
    }

    pub(crate) async fn get_authed<T: serde::de::DeserializeOwned>(
        &self,
        path: String,
        context: &'static str,
    ) -> ApiResult<T> {
        let resp = self
            .inner
            .get(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status(resp, context).await
    }

    pub(crate) async fn post_authed_json<R: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: String,
        body: &R,
        context: &'static str,
    ) -> ApiResult<T> {
        let resp = self
            .inner
            .post(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .json(body)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status(resp, context).await
    }

    /// POST with JSON body and no expected response body (server returns
    /// `204 No Content`). Used for fire-and-forget audit writes and
    /// similar one-way admin actions.
    pub(crate) async fn post_authed_no_content<R: serde::Serialize>(
        &self,
        path: &str,
        context: &'static str,
        body: &R,
    ) -> ApiResult<()> {
        let resp = self
            .inner
            .post(self.url(path))
            .bearer_auth(&self.auth_bearer)
            .json(body)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(CoreApiError::Internal(format!(
                "{context} bad status: {}",
                resp.status()
            )))
        }
    }

    pub(crate) async fn post_authed_no_body<T: serde::de::DeserializeOwned>(
        &self,
        path: String,
        context: &'static str,
    ) -> ApiResult<T> {
        let resp = self
            .inner
            .post(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status(resp, context).await
    }

    pub(crate) async fn put_authed_json<R: serde::Serialize>(
        &self,
        path: String,
        body: &R,
        context: &'static str,
    ) -> ApiResult<()> {
        let resp = self
            .inner
            .put(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .json(body)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status_unit(resp, context).await
    }

    pub(crate) async fn put_authed_json_returning<
        R: serde::Serialize,
        T: serde::de::DeserializeOwned,
    >(
        &self,
        path: String,
        body: &R,
        context: &'static str,
    ) -> ApiResult<T> {
        let resp = self
            .inner
            .put(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .json(body)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status(resp, context).await
    }

    pub(crate) async fn delete_authed(&self, path: String, context: &'static str) -> ApiResult<()> {
        let resp = self
            .inner
            .delete(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status_unit(resp, context).await
    }

    pub(crate) fn enc(part: &str) -> String {
        // axum's matchit accepts most strings unescaped, but addresses
        // contain `@` and threads contain `:` etc. Percent-encode anything
        // not in the unreserved set.
        const RESERVED: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
            .add(b' ')
            .add(b'@')
            .add(b'/')
            .add(b':')
            .add(b'#')
            .add(b'?')
            .add(b'+')
            .add(b'%');
        percent_encoding::utf8_percent_encode(part, RESERVED).to_string()
    }

    // ── health ──────────────────────────────────────────────────────

    /// Healthz probe — NO auth (LB-reachable).
    pub async fn healthz(&self) -> ApiResult<types::HealthResponse> {
        let resp = self
            .inner
            .get(self.url(method::health::PATH_HEALTHZ))
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("healthz transport: {e}")))?;
        Self::map_status(resp, "healthz").await
    }

    /// Readyz — NO auth.
    pub async fn readyz(&self) -> ApiResult<types::HealthResponse> {
        let resp = self
            .inner
            .get(self.url(method::health::PATH_READYZ))
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("readyz transport: {e}")))?;
        Self::map_status(resp, "readyz").await
    }

    // ── conversation (Rock 1 + Rock 2) ──────────────────────────────

    // ── thread read ─────────────────────────────────────────────────

    // ── thread mutate ───────────────────────────────────────────────

    // ── mailbox CRUD ────────────────────────────────────────────────

    // ── message read ────────────────────────────────────────────────

    // ── analysis ────────────────────────────────────────────────────

    // ── admin auth hot path ─────────────────────────────────────────

    // ── outbound queue (sender ↔ core) ──────────────────────────────

    // ── local aliases (fastcore-embedded kevy) ──────────────────────
}

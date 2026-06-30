//! Unified error model for the mailrs-core-api wire contract.
//!
//! Serialized as JSON `{"code": "<variant>", "message": "..."}` over HTTP.
//! HTTP status code is derived from the variant.

use serde::{Deserialize, Serialize};

/// Errors that any RPC method may return.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", content = "message")]
pub enum CoreApiError {
    /// Resource not found (404). Includes the kind of resource.
    #[error("not found: {0}")]
    NotFound(String),

    /// Conflict on uniqueness or invariant (409).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Client passed bad input (400).
    #[error("bad request: {0}")]
    BadRequest(String),

    /// Missing or invalid `Authorization: Bearer <secret>` (401).
    #[error("unauthorized")]
    Unauthorized,

    /// Authenticated but operation forbidden (403).
    #[error("forbidden")]
    Forbidden,

    /// Backend (PG/SPG/kevy) pool exhausted (503) — cascade signal.
    /// fastcore signals this for the same reason `connect_pool_with_retry`
    /// today emits "running in degraded mode": the wire-level expectation
    /// is the same regardless of which backend is running.
    #[error("pool exhausted")]
    PoolExhausted,

    /// Backend doesn't support this feature (501).
    /// fastcore returns this for `semantic_search`, `search_conversations`
    /// (PG FTS fallback), etc. — webapi must tolerate and degrade gracefully.
    #[error("backend unsupported")]
    BackendUnsupported,

    /// Backend timeout (504).
    #[error("backend timeout")]
    Timeout,

    /// Catch-all internal error (500).
    #[error("internal: {0}")]
    Internal(String),
}

impl CoreApiError {
    /// HTTP status code for this error variant.
    pub fn status_code(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::Conflict(_) => 409,
            Self::BadRequest(_) => 400,
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::PoolExhausted => 503,
            Self::BackendUnsupported => 501,
            Self::Timeout => 504,
            Self::Internal(_) => 500,
        }
    }
}

/// Result type used by client + server method definitions.
pub type ApiResult<T> = std::result::Result<T, CoreApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_mapping() {
        assert_eq!(CoreApiError::NotFound("user".into()).status_code(), 404);
        assert_eq!(CoreApiError::PoolExhausted.status_code(), 503);
        assert_eq!(CoreApiError::BackendUnsupported.status_code(), 501);
    }

    #[test]
    fn serialization_roundtrip() {
        let err = CoreApiError::NotFound("user@example.com".into());
        let s = serde_json::to_string(&err).unwrap();
        let back: CoreApiError = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, CoreApiError::NotFound(ref m) if m == "user@example.com"));
    }

    #[test]
    fn unit_variant_serialization() {
        let err = CoreApiError::Unauthorized;
        let s = serde_json::to_string(&err).unwrap();
        let back: CoreApiError = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, CoreApiError::Unauthorized));
    }
}

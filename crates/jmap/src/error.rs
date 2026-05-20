//! JMAP standard method errors (RFC 8620 §3.6.2).
//!
//! Each variant serialises into the `{"type": "...", "description": "..."}`
//! shape JMAP clients expect. The dispatcher converts a handler's
//! `Result<Value, JmapMethodError>` into that shape automatically.

use serde_json::Value;

/// Subset of standard JMAP method errors used by this crate's handlers.
///
/// JMAP defines more (e.g. `cannotCalculateChanges`, `tooLarge`) — feel free
/// to extend; the `Other` variant is the escape hatch for anything not yet
/// modelled.
#[derive(Debug, Clone)]
pub enum JmapMethodError {
    /// RFC 8620 §3.6.2 — `serverFail` with an opaque description.
    ServerFail(String),
    /// RFC 8620 §3.6.2 — store is configured but currently unreachable.
    ServerUnavailable(String),
    /// RFC 8620 §3.6.2 — required arg missing or shape wrong.
    InvalidArguments(String),
    /// RFC 8620 §3.6.2 — method name not recognised.
    UnknownMethod(String),
    /// RFC 8620 §3.6.2 — caller not authorised for this account.
    AccountNotFound,
    /// Anything else; the first string is the error `type` value, second the
    /// human description.
    Other { kind: String, description: String },
}

impl JmapMethodError {
    /// Render to the canonical JMAP error JSON shape.
    pub fn to_json(&self) -> Value {
        let (kind, description) = match self {
            JmapMethodError::ServerFail(d) => ("serverFail", d.as_str()),
            JmapMethodError::ServerUnavailable(d) => ("serverUnavailable", d.as_str()),
            JmapMethodError::InvalidArguments(d) => ("invalidArguments", d.as_str()),
            JmapMethodError::UnknownMethod(d) => ("unknownMethod", d.as_str()),
            JmapMethodError::AccountNotFound => ("accountNotFound", ""),
            JmapMethodError::Other { kind, description } => (kind.as_str(), description.as_str()),
        };
        if description.is_empty() {
            serde_json::json!({ "type": kind })
        } else {
            serde_json::json!({ "type": kind, "description": description })
        }
    }
}

impl std::fmt::Display for JmapMethodError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JmapMethodError::ServerFail(d) => write!(f, "serverFail: {d}"),
            JmapMethodError::ServerUnavailable(d) => write!(f, "serverUnavailable: {d}"),
            JmapMethodError::InvalidArguments(d) => write!(f, "invalidArguments: {d}"),
            JmapMethodError::UnknownMethod(d) => write!(f, "unknownMethod: {d}"),
            JmapMethodError::AccountNotFound => write!(f, "accountNotFound"),
            JmapMethodError::Other { kind, description } => write!(f, "{kind}: {description}"),
        }
    }
}

impl std::error::Error for JmapMethodError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_fail_serialises_with_description() {
        let err = JmapMethodError::ServerFail("boom".into());
        let v = err.to_json();
        assert_eq!(v["type"], "serverFail");
        assert_eq!(v["description"], "boom");
    }

    #[test]
    fn account_not_found_has_no_description() {
        let err = JmapMethodError::AccountNotFound;
        let v = err.to_json();
        assert_eq!(v["type"], "accountNotFound");
        assert!(v.get("description").is_none());
    }

    #[test]
    fn other_variant_uses_kind_and_description() {
        let err = JmapMethodError::Other {
            kind: "tooLarge".into(),
            description: "1 MB > 100 KB".into(),
        };
        let v = err.to_json();
        assert_eq!(v["type"], "tooLarge");
        assert_eq!(v["description"], "1 MB > 100 KB");
    }
}

//! Agent-key scope vocabulary and the REST enforcement rule.
//!
//! Vocabulary: `admin` (everything), `mail:write` (read + write, no
//! /api/admin), `mail:read` (GET/HEAD only, no /api/admin), `mock` (only
//! /api/mock/…), `mock:{ns}` (only that one mock namespace, cannot create
//! or delete it). An empty scope list grants full owner access; only keys
//! made before scopes were required have one, and no new key may. A key
//! with several scopes gets the union of what each one allows. No key, of
//! any scope, can create or revoke keys: that takes a signed-in session.

use axum::http::{Method, StatusCode};

const MOCK_PREFIX: &str = "/api/mock";
const MOCK_NAMESPACES: &str = "/api/mock/namespaces";

/// Scopes of the caller, inserted by the session middleware next to
/// `AuthedUser` on every authenticated request. Empty for a browser session
/// and for an unscoped agent key, both of which mean full owner access.
#[derive(Debug, Clone, Default)]
pub struct AgentScopes(pub Vec<String>);

impl AgentScopes {
    /// See [`mock_ns_restriction`].
    pub fn mock_ns_restriction(&self) -> Option<Vec<String>> {
        mock_ns_restriction(&self.0)
    }
}

/// True when `scope` is in the vocabulary `create_agent_key` accepts.
pub fn is_known_scope(scope: &str) -> bool {
    match scope {
        "admin" | "mail:read" | "mail:write" | "mock" => true,
        _ => scope
            .strip_prefix("mock:")
            .is_some_and(|ns| mailrs_mock::validate_namespace(ns).is_ok()),
    }
}

/// 400 unless there is at least one scope and every scope is in the
/// vocabulary: a new key names what it may do.
pub fn validate_agent_scopes(scopes: &[String]) -> Result<(), StatusCode> {
    match !scopes.is_empty() && scopes.iter().all(|s| is_known_scope(s)) {
        true => Ok(()),
        false => Err(StatusCode::BAD_REQUEST),
    }
}

/// Which mock namespaces the caller may touch. `None` = every namespace the
/// owner can reach (empty scopes, or any of `admin` / `mock` / `mail:*`);
/// `Some(list)` = only the namespaces named by its `mock:{ns}` scopes.
/// Holders of `Some(_)` can neither create nor delete namespaces, and a
/// namespace listing must be filtered down to `list`.
pub fn mock_ns_restriction(scopes: &[String]) -> Option<Vec<String>> {
    let unrestricted = scopes.is_empty()
        || scopes
            .iter()
            .any(|s| matches!(s.as_str(), "admin" | "mock" | "mail:read" | "mail:write"));
    if unrestricted {
        return None;
    }
    Some(
        scopes
            .iter()
            .filter_map(|s| s.strip_prefix("mock:"))
            .map(str::to_string)
            .collect(),
    )
}

/// Where keys are made and revoked.
const AGENT_KEYS: &str = "/api/agent/keys";

pub(super) fn agent_scopes_allow(scopes: &[String], method: &Method, path: &str) -> bool {
    if under(path, AGENT_KEYS) {
        return false;
    }
    scopes.is_empty() || scopes.iter().any(|s| scope_allows(s, method, path))
}

fn scope_allows(scope: &str, method: &Method, path: &str) -> bool {
    let admin_path = path.starts_with("/api/admin");
    match scope {
        "admin" => true,
        "mail:write" => !admin_path,
        "mail:read" => !admin_path && matches!(*method, Method::GET | Method::HEAD),
        "mock" => under(path, MOCK_PREFIX),
        _ => match scope.strip_prefix("mock:") {
            Some(ns) if mailrs_mock::validate_namespace(ns).is_ok() => {
                mock_ns_allows(ns, method, path)
            }
            _ => false,
        },
    }
}

fn mock_ns_allows(ns: &str, method: &Method, path: &str) -> bool {
    if path == MOCK_NAMESPACES {
        // the listing handler filters by `mock_ns_restriction`
        return *method == Method::GET;
    }
    let base = format!("{MOCK_NAMESPACES}/{ns}");
    match path.strip_prefix(base.as_str()) {
        Some("") => *method != Method::DELETE,
        Some(rest) => rest.starts_with('/'),
        None => false,
    }
}

fn under(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn empty_scopes_grant_full_access() {
        assert!(agent_scopes_allow(&[], &Method::POST, "/api/admin/domains"));
        assert!(agent_scopes_allow(&[], &Method::GET, "/api/conversations"));
    }

    #[test]
    fn no_key_manages_keys() {
        for scopes in [s(&[]), s(&["admin"]), s(&["mail:write"]), s(&["mail:read"])] {
            assert!(!agent_scopes_allow(
                &scopes,
                &Method::POST,
                "/api/agent/keys"
            ));
            assert!(!agent_scopes_allow(
                &scopes,
                &Method::GET,
                "/api/agent/keys"
            ));
            assert!(!agent_scopes_allow(
                &scopes,
                &Method::DELETE,
                "/api/agent/keys/3"
            ));
        }
        assert!(agent_scopes_allow(
            &s(&["mail:write"]),
            &Method::GET,
            "/api/agent/keysmith"
        ));
    }

    #[test]
    fn admin_scope_grants_everything() {
        let scopes = s(&["admin"]);
        assert!(agent_scopes_allow(
            &scopes,
            &Method::DELETE,
            "/api/admin/aliases/x"
        ));
    }

    #[test]
    fn mail_read_is_get_only_and_never_admin() {
        let scopes = s(&["mail:read"]);
        assert!(agent_scopes_allow(
            &scopes,
            &Method::GET,
            "/api/conversations"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::POST,
            "/api/mail/send"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::GET,
            "/api/admin/accounts"
        ));
    }

    #[test]
    fn mail_write_allows_writes_but_not_admin() {
        let scopes = s(&["mail:write"]);
        assert!(agent_scopes_allow(&scopes, &Method::POST, "/api/mail/send"));
        assert!(agent_scopes_allow(
            &scopes,
            &Method::GET,
            "/api/conversations"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::GET,
            "/api/admin/accounts"
        ));
    }

    #[test]
    fn unknown_scope_denies_everything() {
        // a key created with scopes: ["read"] (not in the vocabulary)
        // must not silently gain access
        let scopes = s(&["read"]);
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::POST,
            "/api/mail/send"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::GET,
            "/api/conversations"
        ));
    }

    #[test]
    fn mail_scopes_cover_mock_paths() {
        let read = s(&["mail:read"]);
        assert!(agent_scopes_allow(
            &read,
            &Method::GET,
            "/api/mock/namespaces/acme"
        ));
        assert!(!agent_scopes_allow(
            &read,
            &Method::POST,
            "/api/mock/namespaces"
        ));
        let write = s(&["mail:write"]);
        assert!(agent_scopes_allow(
            &write,
            &Method::POST,
            "/api/mock/namespaces"
        ));
    }

    #[test]
    fn mock_scope_is_limited_to_mock_paths() {
        let scopes = s(&["mock"]);
        assert!(agent_scopes_allow(
            &scopes,
            &Method::POST,
            "/api/mock/namespaces"
        ));
        assert!(agent_scopes_allow(
            &scopes,
            &Method::DELETE,
            "/api/mock/namespaces/acme"
        ));
        assert!(agent_scopes_allow(&scopes, &Method::GET, "/api/mock"));
        assert!(!agent_scopes_allow(&scopes, &Method::GET, "/api/mockery"));
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::GET,
            "/api/conversations"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::POST,
            "/api/mail/send"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::GET,
            "/api/admin/accounts"
        ));
    }

    #[test]
    fn mock_ns_scope_is_limited_to_its_namespace() {
        let scopes = s(&["mock:acme"]);
        let m = |method: Method, path: &str| agent_scopes_allow(&scopes, &method, path);
        assert!(m(Method::GET, "/api/mock/namespaces"));
        assert!(!m(Method::POST, "/api/mock/namespaces"));
        assert!(m(Method::GET, "/api/mock/namespaces/acme"));
        assert!(m(Method::PATCH, "/api/mock/namespaces/acme"));
        assert!(!m(Method::DELETE, "/api/mock/namespaces/acme"));
        assert!(m(Method::GET, "/api/mock/namespaces/acme/messages"));
        assert!(m(Method::DELETE, "/api/mock/namespaces/acme/messages/7"));
        assert!(!m(Method::GET, "/api/mock/namespaces/acme2"));
        assert!(!m(Method::GET, "/api/mock/namespaces/other/messages"));
        assert!(!m(Method::GET, "/api/mock/other"));
        assert!(!m(Method::GET, "/api/conversations"));
    }

    #[test]
    fn invalid_mock_ns_scope_grants_nothing() {
        let scopes = s(&["mock:"]);
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::GET,
            "/api/mock/namespaces/"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &Method::GET,
            "/api/mock/namespaces"
        ));
    }

    #[test]
    fn vocabulary() {
        for ok in ["admin", "mail:read", "mail:write", "mock", "mock:acme"] {
            assert!(is_known_scope(ok), "{ok}");
        }
        for bad in ["read", "mock:", "mock:A", "mock:-x", "mail:*", ""] {
            assert!(!is_known_scope(bad), "{bad}");
        }
        assert_eq!(validate_agent_scopes(&s(&["mock", "mock:acme"])), Ok(()));
        assert_eq!(
            validate_agent_scopes(&s(&["mock", "bogus"])),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(validate_agent_scopes(&[]), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn mock_ns_restriction_rules() {
        assert_eq!(mock_ns_restriction(&[]), None);
        assert_eq!(mock_ns_restriction(&s(&["mock"])), None);
        assert_eq!(mock_ns_restriction(&s(&["admin"])), None);
        assert_eq!(mock_ns_restriction(&s(&["mock:a", "mail:read"])), None);
        assert_eq!(
            mock_ns_restriction(&s(&["mock:a", "mock:b"])),
            Some(s(&["a", "b"]))
        );
    }
}

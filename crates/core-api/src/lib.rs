//! Wire contract crate for the mailrs 4-process split.
//!
//! ## Architecture
//!
//! Four binaries communicate via this crate's RPC types:
//!
//! ```text
//!   receiver ──spool+kevy──> core ──HTTP──> webapi
//!                              │             │
//!                              │             ▼
//!                              └──HTTP────  sender
//! ```
//!
//! `core` and `fastcore` are alternate implementations of the same
//! `mailrs-core-api::server` trait — webapi/sender are agnostic to
//! which is running.
//!
//! ## Module map
//!
//! - `error` — `CoreApiError` + HTTP status mapping
//! - `types` — re-exports from `mailrs-mailbox` + wire-only types
//! - `method` — RPC method definitions: path + req/resp types
//! - `client` — `reqwest`-based async client (feature `client`)
//! - `server` — `axum`-based handler scaffolding (feature `server`)

// missing-docs is intentionally NOT denied at the crate level during
// Phase 1 scaffolding — sub-modules are stubs that will gain full doc
// coverage as request/response types fill in (checklist 1.5/1.6).
#![allow(missing_docs)]

pub mod error;
pub mod method;
pub mod types;

#[cfg(feature = "client")]
pub mod client;
// Split out of `client.rs` on 2026-08-02 by subject. Each carries a
// second `impl CoreClient` block, so the methods stay on the one type.
#[cfg(feature = "client")]
mod client_accounts;
#[cfg(feature = "client")]
mod client_labels;
#[cfg(feature = "client")]
mod client_messages;
#[cfg(feature = "client")]
mod client_sidestate;
#[cfg(feature = "client")]
mod client_threads;

#[cfg(feature = "server")]
pub mod server;

/// API version. Bump on breaking change. Path prefix is `/v1/...`.
pub const API_VERSION: &str = "v1";

/// Default port mailrs-core listens on for internal RPC.
/// Override with `MAILRS_CORE_RPC_ADDR` env var.
pub const DEFAULT_CORE_RPC_PORT: u16 = 3300;

/// Env var name carrying the shared internal auth secret.
/// All 4 processes (core/fastcore/webapi/sender/receiver-if-needed) must
/// share the same value. Verified per request via `Authorization: Bearer`.
pub const AUTH_SECRET_ENV: &str = "MAILRS_CORE_API_SECRET";

/// The secret a core needs before it serves its RPC. Unset or blank is an
/// error, not "no authentication": the RPC trusts the user named in each
/// path, so a core reachable without the secret would answer for anyone.
pub fn required_auth_secret() -> Result<String, String> {
    match std::env::var(AUTH_SECRET_ENV) {
        Ok(s) if !s.trim().is_empty() => Ok(s),
        Ok(_) => Err(format!(
            "{AUTH_SECRET_ENV} is blank; the core RPC is not served without it"
        )),
        Err(_) => Err(format!(
            "{AUTH_SECRET_ENV} is not set; the core RPC is not served without it"
        )),
    }
}

/// Secrets read from the environment by one process or another. Each is
/// optional, but one that is set and blank is a mistake in the deployment
/// (an empty value in `.env`), and each reader would otherwise take it as
/// "not configured" and carry on with the feature quietly off.
pub const SECRET_ENVS: &[&str] = &[
    AUTH_SECRET_ENV,
    "MAILRS_APNS_KEY_ID",
    "MAILRS_APNS_KEY_PATH",
    "MAILRS_CORE_SYNC_SECRET_FROM",
    "MAILRS_CORE_SYNC_SECRET_TO",
    "MAILRS_DKIM_KEYS",
    "MAILRS_DKIM_PRIVATE_KEY",
    "MAILRS_DKIM_PRIVATE_KEY_PEM",
    "MAILRS_DKIM_PRIVATE_KEY_PEM_FILE",
    "MAILRS_GOOGLE_MAIL_CLIENT_SECRET",
    "MAILRS_LDAP_BIND_PASSWORD",
    "MAILRS_LLM_API_KEY",
    "MAILRS_MICROSOFT_MAIL_CLIENT_SECRET",
    "MAILRS_OIDC_APPLE_CLIENT_SECRET",
    "MAILRS_OIDC_CLIENT_SECRET",
    "MAILRS_OIDC_GITHUB_CLIENT_SECRET",
    "MAILRS_OIDC_GOOGLE_CLIENT_SECRET",
    "MAILRS_OIDC_MICROSOFT_CLIENT_SECRET",
    "MAILRS_OIDC_PORTAL_CLIENT_SECRET",
    "MAILRS_RESET_CODE_KEY",
    "MAILRS_SRS_SECRET",
    "MAILRS_TLS_KEY",
    "MAILRS_WEBHOOK_API_KEY",
];

/// The secrets in [`SECRET_ENVS`] that are set but blank, by name. A
/// process refuses to start while this is not empty.
pub fn blank_secrets() -> Vec<&'static str> {
    blank_among(SECRET_ENVS, |name| std::env::var(name).ok())
}

fn blank_among(names: &[&'static str], get: impl Fn(&str) -> Option<String>) -> Vec<&'static str> {
    names
        .iter()
        .copied()
        .filter(|name| get(name).is_some_and(|v| v.trim().is_empty()))
        .collect()
}

/// Exit with status 2, naming every blank secret, when there is one.
/// Called first thing at each binary's start.
pub fn refuse_blank_secrets() {
    let blank = blank_secrets();
    if !blank.is_empty() {
        eprintln!(
            "set but blank, refusing to start: {} (remove the line or give it a value)",
            blank.join(", ")
        );
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_set_and_blank_secrets_are_named() {
        let env = |name: &str| match name {
            "A" => Some(String::new()),
            "B" => Some("  ".into()),
            "C" => Some("x".into()),
            _ => None,
        };
        assert_eq!(blank_among(&["A", "B", "C", "D"], env), vec!["A", "B"]);
    }

    #[test]
    fn every_secret_read_in_the_code_is_listed() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut found = std::collections::BTreeSet::new();
        scan(&crates, &mut found);
        assert!(found.contains(AUTH_SECRET_ENV), "the scan found nothing");
        let missing: Vec<_> = found
            .iter()
            .filter(|n| !SECRET_ENVS.contains(&n.as_str()))
            .collect();
        assert!(missing.is_empty(), "add to SECRET_ENVS: {missing:?}");
    }

    fn scan(dir: &std::path::Path, found: &mut std::collections::BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() && path.file_name().is_some_and(|n| n != "target") {
                scan(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for (i, _) in text.match_indices("MAILRS_") {
                    let name: String = text[i..]
                        .chars()
                        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                        .collect();
                    if looks_secret(&name) {
                        found.insert(name);
                    }
                }
            }
        }
    }

    fn looks_secret(name: &str) -> bool {
        name.split('_')
            .any(|part| matches!(part, "SECRET" | "PASSWORD" | "KEY" | "KEYS"))
    }
}

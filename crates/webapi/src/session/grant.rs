//! What signed a long-lived connection in, so it can be asked again.
//!
//! A request is authenticated once; a WebSocket can stay open for days.
//! Every [`RECHECK`] the connection asks whether what let it in still
//! would: the session still exists (signing out, a password change and a
//! revocation all delete it), the agent key is not revoked, the app
//! password is still listed, the account password is the one it signed in
//! with, and the account is still there and active. A connection that
//! fails is closed, so a revoked credential stops reading mail within
//! `RECHECK` plus one check.
//!
//! When the core cannot be asked, the connection is kept and asked again
//! next time: a core restart is not a revocation.

use std::time::Duration;

use axum::http::HeaderMap;
use mailrs_core_api::method::message_index::fnv64;

use crate::WebState;

/// How often an open connection checks its credential.
pub const RECHECK: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grant {
    /// A web session's token.
    Session(String),
    /// An agent key's token.
    AgentKey(String),
    /// An app password, by its id.
    AppPassword(String),
    /// The account password, by a fingerprint of its stored hash.
    Password(u64),
}

/// The account a connection serves and what let it in.
#[derive(Debug, Clone)]
pub struct Held {
    pub user: String,
    pub grant: Grant,
}

pub fn fingerprint(password_hash: &str) -> u64 {
    fnv64(password_hash.as_bytes())
}

impl Held {
    /// A request signed in with a session or agent key token, from its
    /// headers (the `Authorization` header or the session cookie; never
    /// the URL).
    pub fn from_headers(user: &str, headers: &HeaderMap) -> Option<Held> {
        let (token, _) = super::extract_token_with_source(headers)?;
        let grant = match token.starts_with(super::AGENT_KEY_PREFIX) {
            true => Grant::AgentKey(token),
            false => Grant::Session(token),
        };
        Some(Held {
            user: user.to_ascii_lowercase(),
            grant,
        })
    }

    /// Whether this connection may stay open.
    pub async fn still_valid(&self, state: &WebState) -> bool {
        use mailrs_core_api::error::CoreApiError;
        let account = match state.core.get_account_with_hash(&self.user).await {
            Ok(a) => a,
            Err(CoreApiError::NotFound(_)) => return false,
            Err(e) => {
                tracing::warn!(error = %e, user = %self.user, "connection recheck: the core cannot be asked");
                return true;
            }
        };
        if !account.public.active {
            return false;
        }
        match &self.grant {
            Grant::Session(t) => super::resolve_session(t.clone()).await.is_some(),
            Grant::AgentKey(t) => super::resolve_agent_key(t.clone()).await.is_some(),
            Grant::AppPassword(id) => {
                let (user, id) = (self.user.clone(), id.clone());
                crate::handlers::kevy_util::with_kevy(move |c| {
                    Ok(mailrs_auth_guard::app_passwords::list(c, &user)?
                        .iter()
                        .any(|i| i.id == id))
                })
                .unwrap_or(true)
            }
            Grant::Password(fp) => {
                fingerprint(account.password_hash.as_deref().unwrap_or_default()) == *fp
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_comes_from_the_header_or_the_cookie_and_names_its_kind() {
        let mut h = HeaderMap::new();
        h.insert("authorization", "Bearer mk_abc".parse().unwrap());
        assert_eq!(
            Held::from_headers("A@x.test", &h).unwrap().grant,
            Grant::AgentKey("mk_abc".into())
        );
        let mut h = HeaderMap::new();
        h.insert("cookie", "mailrs_session=s1".parse().unwrap());
        let held = Held::from_headers("a@x.test", &h).unwrap();
        assert_eq!(held.grant, Grant::Session("s1".into()));
        assert!(Held::from_headers("a@x.test", &HeaderMap::new()).is_none());
        assert_ne!(fingerprint("$argon2id$a"), fingerprint("$argon2id$b"));
    }
}

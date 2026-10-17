use mailrs_core::users::UserStore;
use mailrs_core_sidestate::families::mock;

use super::ConnectionContext;

/// verify credentials against users.toml first, then the core's accounts
/// (split topology), then PG accounts table, then LDAP
pub(super) async fn verify_credentials(
    ctx: &ConnectionContext,
    username: &str,
    password: &str,
) -> bool {
    // a mock-domain login is a namespace's sandbox and never an account
    if mock::is_mock(username) {
        return verify_namespace_token(ctx, username, password).await;
    }
    if ctx.users.verify(username, password) {
        return true;
    }
    if let Some(ref cs) = ctx.credential_store {
        match cs.password_hash(username).await {
            // argon2 only, as webapi's login and IMAP accept
            Ok(Some(hash)) => {
                if hash.starts_with("$argon2") && UserStore::verify_hash(password, &hash) {
                    return true;
                }
            }
            Ok(None) => mailrs_core::users::dummy_verify(password),
            Err(e) => {
                tracing::warn!(error = %e, user = %username, "submission auth: core lookup failed");
            }
        }
    }
    if let Some(ref ds) = ctx.account_store {
        match ds.password_hash(username).await {
            Ok(Some(hash)) => {
                if hash_matches(password, &hash) {
                    return true;
                }
            }
            _ => {
                // constant-time: do dummy argon2 work even when account not found
                mailrs_core::users::dummy_verify(password);
            }
        }
    }
    // try LDAP as last fallback
    if let Some(ref ldap) = ctx.ldap_config {
        return ldap.authenticate(username, password).await;
    }
    false
}

/// `{namespace}@{mock domain}` with the namespace's SMTP token.
async fn verify_namespace_token(ctx: &ConnectionContext, username: &str, token: &str) -> bool {
    let Some(ns) = sandbox_namespace(username) else {
        return false;
    };
    let Some(client) = ctx.spam_lists_client.clone() else {
        return false;
    };
    let token = token.to_string();
    tokio::task::spawn_blocking(move || {
        client.with_conn(|c| mock::verify_smtp_token(c, &ns, &token).map_err(std::io::Error::other))
    })
    .await
    .is_ok_and(|r| r.unwrap_or(false))
}

/// The namespace a sandbox login names: the bare `{namespace}@{mock domain}`.
pub(super) fn sandbox_namespace(username: &str) -> Option<String> {
    mock::parse(username)
        .filter(|a| a.tag.is_empty())
        .map(|a| a.namespace)
}

fn hash_matches(password: &str, hash: &str) -> bool {
    if hash.is_empty() {
        false
    } else if hash.starts_with("$argon2") {
        UserStore::verify_hash(password, hash)
    } else {
        hash == password
    }
}

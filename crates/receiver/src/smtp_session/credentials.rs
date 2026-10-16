use mailrs_core::users::UserStore;

use super::ConnectionContext;

/// verify credentials against users.toml first, then the core's accounts
/// (split topology), then PG accounts table, then LDAP
pub(super) async fn verify_credentials(
    ctx: &ConnectionContext,
    username: &str,
    password: &str,
) -> bool {
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

fn hash_matches(password: &str, hash: &str) -> bool {
    if hash.is_empty() {
        false
    } else if hash.starts_with("$argon2") {
        UserStore::verify_hash(password, hash)
    } else {
        hash == password
    }
}

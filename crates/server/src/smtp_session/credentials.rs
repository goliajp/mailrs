use crate::users::UserStore;

use super::ConnectionContext;

/// verify credentials against users.toml first, then PG accounts table, then LDAP
pub(super) async fn verify_credentials(
    ctx: &ConnectionContext,
    username: &str,
    password: &str,
) -> bool {
    if ctx.users.verify(username, password) {
        return true;
    }
    if let Some(ref ds) = ctx.domain_store {
        match ds.get_account_with_hash(username).await {
            Ok(Some((_account, hash))) => {
                let valid = if hash.is_empty() {
                    false
                } else if hash.starts_with("$argon2") {
                    UserStore::verify_hash(password, &hash)
                } else {
                    hash == password
                };
                if valid {
                    return true;
                }
            }
            _ => {
                // constant-time: do dummy argon2 work even when account not found
                crate::users::dummy_verify(password);
            }
        }
    }
    // try LDAP as last fallback
    if let Some(ref ldap) = ctx.ldap_config {
        return ldap.authenticate(username, password).await;
    }
    false
}

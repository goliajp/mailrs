use ldap3::{LdapConnAsync, Scope, SearchEntry};

/// LDAP authentication configuration
#[derive(Debug, Clone)]
pub struct LdapConfig {
    pub url: String,
    pub bind_dn: String,
    pub bind_password: String,
    pub base_dn: String,
    pub user_filter: String,
}

impl LdapConfig {
    /// authenticate a user against the LDAP directory
    ///
    /// 1. connect and bind with service account
    /// 2. search for the user by email using user_filter
    /// 3. if found, attempt to bind with the user's DN and password
    /// 4. return true only if the user bind succeeds
    ///
    /// returns false on any error (connection failure, user not found, bad password)
    /// so that LDAP downtime does not block login
    pub async fn authenticate(&self, email: &str, password: &str) -> bool {
        // connect to LDAP server
        let (conn, mut ldap) = match LdapConnAsync::new(&self.url).await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "ldap connection failed");
                return false;
            }
        };

        // drive the connection in background
        ldap3::drive!(conn);

        // bind with service account
        if let Err(e) = ldap
            .simple_bind(&self.bind_dn, &self.bind_password)
            .await
            .and_then(|res| res.success())
        {
            tracing::warn!(error = %e, "ldap service account bind failed");
            let _ = ldap.unbind().await;
            return false;
        }

        // search for user by email
        let filter = self.user_filter.replace("{}", email);
        let search_result = match ldap
            .search(&self.base_dn, Scope::Subtree, &filter, vec!["dn"])
            .await
        {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!(error = %e, email, "ldap user search failed");
                let _ = ldap.unbind().await;
                return false;
            }
        };

        let (entries, _) = match search_result.success() {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, email, "ldap search result error");
                let _ = ldap.unbind().await;
                return false;
            }
        };

        if entries.is_empty() {
            tracing::debug!(email, "ldap user not found");
            let _ = ldap.unbind().await;
            return false;
        }

        let user_dn = SearchEntry::construct(entries.into_iter().next().unwrap()).dn;
        let _ = ldap.unbind().await;

        // open a new connection to bind as the user
        let (conn2, mut ldap2) = match LdapConnAsync::new(&self.url).await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "ldap user bind connection failed");
                return false;
            }
        };
        ldap3::drive!(conn2);

        let user_bind = ldap2.simple_bind(&user_dn, password).await;
        let authenticated = match user_bind {
            Ok(result) => result.success().is_ok(),
            Err(_) => false,
        };

        let _ = ldap2.unbind().await;

        if authenticated {
            tracing::info!(email, "ldap authentication succeeded");
        } else {
            tracing::debug!(email, "ldap authentication failed (bad password)");
        }

        authenticated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_placeholder_replacement() {
        let config = LdapConfig {
            url: "ldap://localhost:389".into(),
            bind_dn: "cn=admin,dc=example,dc=com".into(),
            bind_password: "secret".into(),
            base_dn: "dc=example,dc=com".into(),
            user_filter: "(&(objectClass=person)(mail={}))".into(),
        };

        let filter = config.user_filter.replace("{}", "alice@example.com");
        assert_eq!(filter, "(&(objectClass=person)(mail=alice@example.com))");
    }
}

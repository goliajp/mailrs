//! Tests for `ldap_auth` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

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

//! The kevy layout of the alias table, shared by the embedded store
//! (`mailrs-mailbox-kevy`) and the network one (`mailrs-alias-store-net`)
//! so either can read what the other wrote.

/// One alias: hash `{target, domain, created_at, active}`.
pub fn alias_v2_key(address: &str) -> String {
    format!("mailrs:alias:v2:{address}")
}

/// What every [`alias_v2_key`] starts with; the range indexes cover it.
pub const ALIAS_V2_PREFIX: &[u8] = b"mailrs:alias:v2:";

/// Set of every alias's source address, on the network store: what lists
/// the aliases with commands any Redis-compatible server has. Written with
/// each alias in one MULTI; the embedded store lists through its own index.
pub const ALIAS_SOURCES_KEY: &[u8] = b"mailrs:alias:sources";

/// Range index over the alias hashes' `domain` field.
pub const IDX_ALIASES_BY_DOMAIN: &[u8] = b"aliases_by_domain";

/// Range index over the alias hashes' `target` field — who forwards to an
/// address.
pub const IDX_ALIASES_BY_TARGET: &[u8] = b"aliases_by_target";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_keys_keep_their_prod_spelling() {
        assert_eq!(alias_v2_key("a@x.jp"), "mailrs:alias:v2:a@x.jp");
        assert_eq!(ALIAS_V2_PREFIX, b"mailrs:alias:v2:");
        assert_eq!(ALIAS_SOURCES_KEY, b"mailrs:alias:sources");
        assert_eq!(IDX_ALIASES_BY_DOMAIN, b"aliases_by_domain");
        assert_eq!(IDX_ALIASES_BY_TARGET, b"aliases_by_target");
    }
}

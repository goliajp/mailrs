//! The addresses an account may send as, beyond its own: the ones an
//! administrator granted it, the aliases that point at it and the email
//! groups it belongs to. Both cores build
//! `EffectivePermissionsResponse::send_as` with [`union`], each from its own
//! store of grants and aliases.

/// Sources of the aliases whose target is `address`. A domain catch-all
/// (`@domain`) is inbound routing, not an identity: counting it would let
/// whoever receives the catch-all send as every address in the domain.
pub fn alias_sources_for<'a>(
    aliases: impl IntoIterator<Item = (&'a str, &'a str)>,
    address: &str,
) -> Vec<String> {
    aliases
        .into_iter()
        .filter(|(source, target)| {
            !source.trim_start().starts_with('@') && target.trim().eq_ignore_ascii_case(address)
        })
        .map(|(source, _)| source.trim().to_string())
        .collect()
}

/// Granted, alias and group addresses as one sorted list, each address
/// once (compared ASCII case-insensitively, first spelling kept).
pub fn union(granted: &[String], alias_sources: Vec<String>, groups: Vec<String>) -> Vec<String> {
    let mut all: Vec<String> = granted
        .iter()
        .map(|s| s.trim().to_string())
        .chain(alias_sources)
        .chain(groups)
        .filter(|s| !s.is_empty())
        .collect();
    all.sort_by_key(|s| s.to_ascii_lowercase());
    all.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    all
}

/// A grant list as an administrator may store it: trimmed, each address
/// once, nothing that is not `local@domain`. `Err` names the first entry
/// that is not an address.
pub fn normalize_grants(list: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(list.len());
    for raw in list {
        let s = raw.trim();
        let Some((local, domain)) = s.split_once('@') else {
            return Err(raw.clone());
        };
        if local.is_empty()
            || domain.is_empty()
            || domain.contains('@')
            || s.contains(char::is_whitespace)
        {
            return Err(raw.clone());
        }
        out.push(s.to_string());
    }
    Ok(union(&out, Vec::new(), Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_count_but_the_catch_all_does_not() {
        let aliases = [
            ("ceo@x.test", "lihao@x.test"),
            ("@x.test", "lihao@x.test"),
            ("sales@x.test", "bob@x.test"),
            ("lab@x.test", "LIHAO@x.test"),
        ];
        assert_eq!(
            alias_sources_for(aliases, "lihao@x.test"),
            vec!["ceo@x.test", "lab@x.test"]
        );
    }

    #[test]
    fn the_union_is_sorted_and_says_each_address_once() {
        let got = union(
            &["noreply@x.test".into(), "Team@x.test".into()],
            vec!["team@x.test".into(), "ceo@x.test".into()],
            vec!["all@x.test".into()],
        );
        assert_eq!(
            got,
            vec!["all@x.test", "ceo@x.test", "noreply@x.test", "Team@x.test"]
        );
    }

    #[test]
    fn grants_must_be_addresses() {
        assert_eq!(
            normalize_grants(&[" noreply@x.test ".into(), "noreply@x.test".into()]),
            Ok(vec!["noreply@x.test".to_string()])
        );
        for bad in ["noreply", "@x.test", "a@", "a b@x.test", "a@b@c"] {
            assert_eq!(
                normalize_grants(&[bad.into()]),
                Err(bad.to_string()),
                "{bad}"
            );
        }
        assert_eq!(normalize_grants(&[]), Ok(Vec::new()));
    }
}

//! `{namespace}.{tag}@domain`, with `{namespace}+{tag}@domain` accepted
//! too and a bare `{namespace}@domain` meaning the empty tag.

/// The namespace and tag of a mock address, both lowercase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockAddress {
    pub namespace: String,
    pub tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    Length,
    Charset,
    Reserved,
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Length => "a namespace is 3 to 32 characters",
            Self::Charset => "a namespace uses only a-z, 0-9 and -",
            Self::Reserved => "this namespace name is reserved",
        })
    }
}

// role and simulation addresses: a namespace by these names would read
// as a mailbox the deployment answers for
const RESERVED: &[&str] = &[
    "postmaster",
    "abuse",
    "admin",
    "root",
    "mailer-daemon",
    "noreply",
    "no-reply",
    "bounce",
    "ooto",
    "complaint",
    "delay",
    "hostmaster",
    "webmaster",
    "security",
    "support",
];

const MAX_TAG: usize = 31;

pub fn validate_namespace(name: &str) -> Result<(), NameError> {
    if !(3..=32).contains(&name.len()) {
        return Err(NameError::Length);
    }
    let ok = name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !ok || name.starts_with('-') || name.ends_with('-') {
        return Err(NameError::Charset);
    }
    if RESERVED.contains(&name) {
        return Err(NameError::Reserved);
    }
    Ok(())
}

pub fn validate_tag(tag: &str) -> bool {
    tag.len() <= MAX_TAG
        && tag
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// Whether `addr` (any case, angle brackets allowed) is on `domain`.
pub fn is_mock_domain(addr: &str, domain: &str) -> bool {
    if domain.is_empty() {
        return false;
    }
    let addr = addr.trim().trim_matches(['<', '>']);
    addr.rsplit_once('@')
        .is_some_and(|(_, d)| d.eq_ignore_ascii_case(domain))
}

/// The namespace and tag of `addr`, or `None` when it is not on `domain`
/// or its local part does not follow the grammar. Some services change
/// the case of an address, so the local part is folded first.
pub fn parse_address(addr: &str, domain: &str) -> Option<MockAddress> {
    if !is_mock_domain(addr, domain) {
        return None;
    }
    let addr = addr.trim().trim_matches(['<', '>']);
    let (local, _) = addr.rsplit_once('@')?;
    let local = local.to_ascii_lowercase();
    if local.len() > 64 {
        return None;
    }
    let (namespace, tag) = match local.find(['.', '+']) {
        Some(i) => (&local[..i], &local[i + 1..]),
        None => (&local[..], ""),
    };
    validate_namespace(namespace).ok()?;
    if !validate_tag(tag) {
        return None;
    }
    Some(MockAddress {
        namespace: namespace.to_string(),
        tag: tag.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: &str = "mock.example.test";

    fn parsed(addr: &str) -> Option<(String, String)> {
        parse_address(addr, D).map(|a| (a.namespace, a.tag))
    }

    #[test]
    fn dot_plus_and_bare_forms() {
        let want = Some(("insight".to_string(), "signup-0417".to_string()));
        assert_eq!(parsed("insight.signup-0417@mock.example.test"), want);
        assert_eq!(parsed("insight+signup-0417@mock.example.test"), want);
        assert_eq!(
            parsed("insight@mock.example.test"),
            Some(("insight".into(), "".into()))
        );
    }

    #[test]
    fn a_tag_may_carry_dots_and_case_is_folded() {
        assert_eq!(
            parsed("<Insight.A.B@MOCK.example.test>"),
            Some(("insight".into(), "a.b".into()))
        );
    }

    #[test]
    fn other_domains_and_bad_names_are_not_mock() {
        assert_eq!(parsed("insight.x@example.test"), None);
        assert_eq!(parsed("ab.x@mock.example.test"), None);
        assert_eq!(parsed("postmaster@mock.example.test"), None);
        assert_eq!(parsed("insight.a/b@mock.example.test"), None);
        assert_eq!(
            parsed(&format!("insight.{}@mock.example.test", "x".repeat(32))),
            None
        );
        assert!(!is_mock_domain("a@mock.example.test", ""));
    }

    #[test]
    fn namespace_rules() {
        assert_eq!(validate_namespace("my-app"), Ok(()));
        assert_eq!(validate_namespace("ab"), Err(NameError::Length));
        assert_eq!(validate_namespace("My-app"), Err(NameError::Charset));
        assert_eq!(validate_namespace("-app"), Err(NameError::Charset));
        assert_eq!(validate_namespace("abuse"), Err(NameError::Reserved));
    }
}

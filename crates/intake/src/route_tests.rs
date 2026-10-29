//! The routing order and group expansion against a directory held in
//! memory, so each rule is checked without either core's store.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use mailrs_core_sidestate::families::email_groups::Group;

use crate::group::{Target, targets};
use crate::route::{Directory, Route, base_address, route, srs_original_with};

#[derive(Default)]
struct Dir {
    root: String,
    accounts: Vec<&'static str>,
    aliases: HashMap<&'static str, &'static str>,
    groups: Vec<Group>,
    domains: Vec<String>,
    broken: bool,
}

impl Directory for Dir {
    fn maildir_root(&self) -> &str {
        &self.root
    }
    fn is_account(&self, addr: &str) -> io::Result<bool> {
        if self.broken {
            return Err(io::Error::other("store away"));
        }
        Ok(self.accounts.contains(&addr))
    }
    // an `@domain` key is that domain's catch-all, as both cores store it
    fn alias(&self, addr: &str) -> io::Result<Option<String>> {
        let domain = addr.rsplit_once('@').map(|(_, d)| format!("@{d}"));
        Ok(self
            .aliases
            .get(addr)
            .or_else(|| domain.and_then(|d| self.aliases.get(d.as_str())))
            .map(|t| t.to_string()))
    }
    fn group(&self, addr: &str) -> io::Result<Option<Group>> {
        Ok(self.groups.iter().find(|g| g.address == addr).cloned())
    }
    fn local_domains(&self) -> Arc<Vec<String>> {
        Arc::new(self.domains.clone())
    }
}

fn group(address: &str, members: &[&str]) -> Group {
    Group {
        id: 1,
        address: address.into(),
        members: members.iter().map(|m| m.to_string()).collect(),
    }
}

fn dir() -> Dir {
    Dir {
        root: "/nonexistent-maildir-root".into(),
        accounts: vec!["bob@golia.jp", "postmaster@golia.jp"],
        domains: vec!["golia.jp".into(), "golia.ai".into()],
        ..Dir::default()
    }
}

#[test]
fn an_account_takes_its_own_mail() {
    assert!(matches!(
        route(&dir(), "bob@golia.jp", "bob@golia.jp").unwrap(),
        Route::Own
    ));
}

/// A catch-all answers for every address without a mailbox, so a group
/// asked after it would never get its mail.
#[test]
fn a_group_is_asked_before_the_alias_table() {
    let mut d = dir();
    d.groups.push(group("team@golia.jp", &["bob@golia.jp"]));
    d.aliases.insert("team@golia.jp", "bob@golia.jp");
    assert!(matches!(
        route(&d, "team@golia.jp", "team@golia.jp").unwrap(),
        Route::Group(_)
    ));
}

#[test]
fn an_alias_reaches_its_target_only_when_the_target_is_a_mailbox() {
    let mut d = dir();
    d.aliases.insert("sales@golia.jp", "BOB@golia.jp");
    d.aliases.insert("dead@golia.jp", "nobody@golia.jp");
    match route(&d, "sales@golia.jp", "sales@golia.jp").unwrap() {
        Route::Alias(t) => assert_eq!(t, "bob@golia.jp"),
        _ => panic!("the alias did not route to its target"),
    }
    assert!(matches!(
        route(&d, "dead@golia.jp", "dead@golia.jp").unwrap(),
        Route::Nobody
    ));
}

#[test]
fn postmaster_on_a_second_domain_reaches_the_first() {
    match route(&dir(), "postmaster@golia.ai", "postmaster@golia.ai").unwrap() {
        Route::Alias(t) => assert_eq!(t, "postmaster@golia.jp"),
        _ => panic!("postmaster on a hosted domain went nowhere"),
    }
    // not one of ours: no fallback
    assert!(matches!(
        route(&dir(), "postmaster@example.com", "postmaster@example.com").unwrap(),
        Route::Nobody
    ));
}

/// An unreadable store must not read as "nobody": the RCPT check would
/// refuse a real address while the store is briefly away.
#[test]
fn a_store_that_cannot_be_read_is_an_error() {
    let d = Dir {
        broken: true,
        ..dir()
    };
    assert!(route(&d, "bob@golia.jp", "bob@golia.jp").is_err());
}

/// Local members are delivered, outside members forwarded, a nested group
/// flattened once, and a mailbox reached twice counted once.
#[test]
fn a_group_expands_to_local_outside_and_nested_members() {
    let tmp = tempfile::tempdir().unwrap();
    let mut d = dir();
    d.root = tmp.path().to_string_lossy().into_owned();
    d.aliases.insert("robert@golia.jp", "bob@golia.jp");
    d.groups.push(group(
        "inner@golia.jp",
        &["postmaster@golia.jp", "ghost@golia.jp"],
    ));
    let outer = group(
        "team@golia.jp",
        &[
            "Bob@golia.jp",
            "robert@golia.jp",
            "friend@example.com",
            "inner@golia.jp",
        ],
    );
    let got: Vec<String> = targets(&d, &outer)
        .into_iter()
        .map(|t| match t {
            Target::Local { mailbox, .. } => format!("local {mailbox}"),
            Target::Outside(m) => format!("outside {m}"),
            Target::Unresolved(m) => format!("unresolved {m}"),
        })
        .collect();
    assert_eq!(
        got,
        [
            "local bob@golia.jp",
            "outside friend@example.com",
            "local postmaster@golia.jp",
            "unresolved ghost@golia.jp",
        ]
    );
}

fn routed(d: &Dir, addr: &str) -> String {
    match route(d, addr, addr).unwrap() {
        Route::Own => "own".into(),
        Route::Alias(t) => format!("alias {t}"),
        Route::Group(g) => format!("group {}", g.address),
        Route::Srs(o) => format!("srs {o}"),
        Route::Nobody => "nobody".into(),
    }
}

#[test]
fn a_tagged_address_reaches_its_base_before_the_catch_all() {
    let mut d = dir();
    d.accounts.push("lihao@golia.jp");
    d.aliases.insert("@golia.jp", "lihao@golia.jp");
    d.aliases.insert("sales@golia.jp", "bob@golia.jp");
    d.groups.push(group("team@golia.jp", &["bob@golia.jp"]));
    assert_eq!(routed(&d, "bob+qip18@golia.jp"), "alias bob@golia.jp");
    assert_eq!(routed(&d, "bob+a+b@golia.jp"), "alias bob@golia.jp");
    assert_eq!(routed(&d, "sales+x@golia.jp"), "alias bob@golia.jp");
    assert_eq!(routed(&d, "team+x@golia.jp"), "group team@golia.jp");
    // an unknown base still goes where the unknown address would
    assert_eq!(routed(&d, "ghost+x@golia.jp"), "alias lihao@golia.jp");
}

#[test]
fn a_tag_on_an_unknown_base_without_catch_all_is_nobody() {
    let d = dir();
    assert_eq!(routed(&d, "ghost+x@golia.jp"), "nobody");
    assert_eq!(routed(&d, "+x@golia.jp"), "nobody");
}

#[test]
fn only_the_first_plus_splits_the_tag() {
    assert_eq!(base_address("a+b+c@x.jp").as_deref(), Some("a@x.jp"));
    assert_eq!(base_address("a@x.jp"), None);
    assert_eq!(base_address("+b@x.jp"), None);
    assert_eq!(base_address("not-an-address"), None);
}

#[test]
fn an_srs_address_reverses_only_under_a_real_key() {
    let forged = mailrs_srs::rewrite("victim@example.com", "golia.jp", "");
    assert_eq!(srs_original_with(&forged, Some("")), None);
    assert_eq!(srs_original_with(&forged, None), None);

    let ours = mailrs_srs::rewrite("friend@example.com", "golia.jp", "k3y");
    assert_eq!(
        srs_original_with(&ours, Some("k3y")).as_deref(),
        Some("friend@example.com")
    );
    assert_eq!(srs_original_with(&forged, Some("k3y")), None);
}

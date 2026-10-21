//! Every path the core-api client can ask for was asked of both cores.
//!
//! Without this, the comparison is only as wide as whoever wrote the
//! scenarios remembered to make it: a method nobody exercised agrees by
//! default. The paths are read from the client's source, the same way
//! `scripts/check-core-contract.sh` reads them, so a new client method
//! fails here until a scenario calls it.

use std::collections::BTreeSet;

/// `/v1/...` string literals in the client, placeholders as `*`, query
/// strings dropped.
pub(super) fn client_paths() -> BTreeSet<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../core-api/src");
    let mut out = BTreeSet::new();
    for entry in std::fs::read_dir(dir).expect("core-api/src") {
        let path = entry.expect("entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name.starts_with("client") && name.ends_with(".rs")) {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("client source");
        for (at, _) in src.match_indices("\"/v1") {
            let lit = &src[at + 1..];
            let end = lit.find('"').expect("closing quote");
            out.insert(normalise(&lit[..end]));
        }
    }
    out
}

fn normalise(path: &str) -> String {
    let path = path.split('?').next().unwrap_or("");
    path.trim_end_matches('/')
        .split('/')
        .map(|seg| if seg.contains('{') { "*" } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

/// A `*` the client builds at runtime matches whatever the route has in
/// that place; a literal segment must match the route's literal.
fn served_by(asked: &str, route: &str) -> bool {
    let a: Vec<&str> = asked.trim_matches('/').split('/').collect();
    let r: Vec<&str> = route.trim_matches('/').split('/').collect();
    a.len() == r.len() && a.iter().zip(&r).all(|(x, y)| *x == "*" || x == y)
}

/// Client paths that no recorded hit answers.
pub(super) fn unexercised(asked: &BTreeSet<String>, hits: &BTreeSet<String>) -> Vec<String> {
    asked
        .iter()
        .filter(|p| !hits.iter().any(|h| served_by(p, h)))
        .cloned()
        .collect()
}

#[test]
fn the_extraction_sees_the_client() {
    let paths = client_paths();
    assert!(paths.len() >= 40, "only {} paths read", paths.len());
    assert!(
        paths.contains("/v1/users/*/conversations:search"),
        "{paths:?}"
    );
}

#[test]
fn a_literal_segment_must_match() {
    assert!(served_by(
        "/v1/users/*/threads/*/read",
        "/v1/users/{user}/threads/{thread_id}/read"
    ));
    assert!(!served_by(
        "/v1/users/*/threads/*/read",
        "/v1/users/{user}/threads/{thread_id}/star"
    ));
    assert!(!served_by(
        "/v1/users/*/threads",
        "/v1/users/{user}/threads/{thread_id}"
    ));
}

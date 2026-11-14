//! Every call the core-api client can make was made of both cores, and
//! answered with success: method and path, so the DELETE half of a URL whose
//! GET a scenario made is still owed, and a 404 does not count.
//!
//! Without this, the comparison is only as wide as whoever wrote the
//! scenarios remembered to make it: a method nobody exercised agrees by
//! default. The paths are read from the client's source, the same way
//! `scripts/check-core-contract.sh` reads them, so a new client method
//! fails here until a scenario calls it.

use std::collections::BTreeSet;

/// `METHOD /v1/...` for each `/v1` string literal in the client: the path
/// with placeholders as `*` and query strings dropped, the method from the
/// request around it.
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
            let method = method_at(&src, at)
                .unwrap_or_else(|| panic!("no request method around {}", &lit[..end]));
            out.insert(format!("{method} {}", normalise(&lit[..end])));
        }
    }
    out
}

/// The method of the request a path literal at `at` is built for: a
/// `.post(self.url(` just before it, or else the first request helper after
/// it (`get_authed`, `post_authed_json`, ...).
fn method_at(src: &str, at: usize) -> Option<&'static str> {
    const VERBS: [(&str, &str); 5] = [
        ("get", "GET"),
        ("post", "POST"),
        ("put", "PUT"),
        ("delete", "DELETE"),
        ("patch", "PATCH"),
    ];
    // the statement the literal sits in, up to it: `self.get_authed("/v1..`
    // or `.post(self.url("/v1..`
    let stmt_start = src[..at].rfind([';', '{']).map_or(0, |i| i + 1);
    let before = &src[stmt_start..at];
    for (verb, method) in VERBS {
        if before.contains(&format!(".{verb}_authed"))
            || before.contains(&format!(".{verb}(self.url("))
        {
            return Some(method);
        }
    }
    let after = &src[at..];
    let next_fn = after.find("\n    pub async fn ").unwrap_or(after.len());
    let body = &after[..next_fn];
    VERBS
        .iter()
        .filter_map(|(verb, method)| {
            let helper = body.find(&format!(".{verb}_authed")).map(|i| (i, *method));
            let raw = body
                .find(&format!(".{verb}(self.url("))
                .map(|i| (i, *method));
            helper.into_iter().chain(raw).min()
        })
        .min()
        .map(|(_, m)| m)
}

fn normalise(path: &str) -> String {
    let path = path.split('?').next().unwrap_or("");
    path.trim_end_matches('/')
        .split('/')
        .map(|seg| if seg.contains('{') { "*" } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

/// Same method, and a `*` the client builds at runtime matches whatever
/// the route has in that place; a literal segment must match the route's
/// literal.
fn served_by(asked: &str, route: &str) -> bool {
    let (Some((am, asked)), Some((rm, route))) = (asked.split_once(' '), route.split_once(' '))
    else {
        return false;
    };
    if am != rm {
        return false;
    }
    let a: Vec<&str> = asked.trim_matches('/').split('/').collect();
    let r: Vec<&str> = route.trim_matches('/').split('/').collect();
    a.len() == r.len() && a.iter().zip(&r).all(|(x, y)| *x == "*" || x == y)
}

/// Calls the run of every client call does not make, each with the test
/// that makes it on cores of its own, because it needs state that run does
/// not set up. That test asserts them with [`require`], so an entry here is
/// a claim something checks.
pub(super) const ELSEWHERE: &[(&str, &str)] = &[
    ("POST /v1/users/*/messages/*/report", "reports"),
    ("GET /v1/users/*/reports/*", "reports"),
    ("POST /v1/admin/reports/*/confirm", "reports"),
    ("POST /v1/admin/reports/*/dismiss", "reports-dismiss"),
];

/// Calls the SQL core does not answer with success yet, each with the gap it
/// is. An entry that both cores come to answer fails the run until it is
/// taken off, so the list only shrinks.
pub(super) const OWED: &[(&str, &str)] = &[
    (
        "POST /v1/admin/rejudge-batches/*/release",
        "the SQL core does not re-judge yet, so it has no rounds (H2)",
    ),
    (
        "POST /v1/admin/rejudge-batches/*/confirm",
        "the SQL core does not re-judge yet, so it has no rounds (H2)",
    ),
    (
        "POST /v1/admin/rejudge-batches/*/discard",
        "the SQL core does not re-judge yet, so it has no rounds (H2)",
    ),
];

/// Calls that succeed only on one store's own kind of damage, which the
/// other store cannot hold; their success is tested on that store itself.
pub(super) const ONE_STORE: &[(&str, &str)] = &[(
    "POST /v1/admin/ownerless-threads/*/delete",
    "a conversation nobody holds exists only in the kevy store; mailbox-kevy's ownerless tests delete one",
)];

/// The [`ELSEWHERE`] calls `owner` makes that one of the two cores did not
/// answer with success.
pub(super) fn require(owner: &str, c: &super::harness::Cores) -> Vec<String> {
    let mut missing = Vec::new();
    for (core, hits) in [("fastcore", &c.fastcore_hits), ("pg-core", &c.pg_core_hits)] {
        let hits = hits.lock().expect("hits").clone();
        for (asked, _) in ELSEWHERE.iter().filter(|(_, o)| *o == owner) {
            if !hits.iter().any(|h| served_by(asked, h)) {
                missing.push(format!("{core}: {owner} did not reach {asked}"));
            }
        }
    }
    missing
}

/// [`OWED`] calls both cores now answer with success: the gap is closed
/// and the entry is to go.
pub(super) fn paid(fastcore: &BTreeSet<String>, pg_core: &BTreeSet<String>) -> Vec<String> {
    OWED.iter()
        .filter(|(p, _)| {
            fastcore.iter().any(|h| served_by(p, h)) && pg_core.iter().any(|h| served_by(p, h))
        })
        .map(|(p, why)| format!("{p} is answered by both cores now; take it off OWED ({why})"))
        .collect()
}

/// Client paths that no recorded hit answers.
pub(super) fn unexercised(asked: &BTreeSet<String>, hits: &BTreeSet<String>) -> Vec<String> {
    asked
        .iter()
        .filter(|p| !ELSEWHERE.iter().any(|(e, _)| e == p))
        .filter(|p| !ONE_STORE.iter().any(|(e, _)| e == p))
        .filter(|p| !OWED.iter().any(|(e, _)| e == p))
        .filter(|p| !hits.iter().any(|h| served_by(p, h)))
        .cloned()
        .collect()
}

#[test]
fn the_extraction_sees_the_client() {
    let paths = client_paths();
    assert!(paths.len() >= 40, "only {} paths read", paths.len());
    assert!(
        paths.contains("POST /v1/users/*/conversations:search"),
        "{paths:?}"
    );
}

#[test]
fn a_literal_segment_must_match() {
    assert!(served_by(
        "POST /v1/users/*/threads/*/read",
        "POST /v1/users/{user}/threads/{thread_id}/read"
    ));
    assert!(!served_by(
        "POST /v1/users/*/threads/*/read",
        "POST /v1/users/{user}/threads/{thread_id}/star"
    ));
    assert!(!served_by(
        "GET /v1/users/*/threads",
        "GET /v1/users/{user}/threads/{thread_id}"
    ));
    assert!(
        !served_by(
            "DELETE /v1/users/*/threads/*",
            "GET /v1/users/{user}/threads/{thread_id}"
        ),
        "another method on the same route"
    );
}

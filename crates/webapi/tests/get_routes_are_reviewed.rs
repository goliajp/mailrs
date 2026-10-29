//! Every GET route has been reviewed for side effects.
//!
//! A page on another site can make a browser send a GET with the session
//! cookie, and the CSRF check only covers the other methods. So a GET that
//! changes something is a hole the CSRF check cannot see. This reads the
//! route table out of the router sources and compares it with
//! `tests/get_routes.txt`, where each GET is listed with what it does beyond
//! reading. A new GET fails here until someone has looked at it.

use std::collections::BTreeSet;
use std::path::Path;

/// The path of every `.route(...)` call that answers GET (`get(` or `any(`).
fn get_routes_in(source: &str) -> BTreeSet<String> {
    // comments may hold parentheses or sit between `.route(` and the path
    let code: String = source
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let bytes = code.as_bytes();
    let mut out = BTreeSet::new();
    let mut from = 0;
    while let Some(at) = code[from..].find(".route(") {
        let start = from + at + ".route(".len();
        let mut depth = 1;
        let mut end = start;
        while depth > 0 && end < bytes.len() {
            match bytes[end] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        let call = &code[start..end.saturating_sub(1)];
        from = end;
        let call = call.trim_start();
        let Some(rest) = call.strip_prefix('"') else {
            continue;
        };
        let Some(close) = rest.find('"') else {
            continue;
        };
        let spec = &rest[close + 1..];
        if spec.contains("get(") || spec.contains("any(") {
            out.insert(rest[..close].to_string());
        }
    }
    out
}

#[test]
fn every_get_route_has_been_reviewed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = vec![root.join("src/router.rs")];
    for entry in std::fs::read_dir(root.join("src/router")).expect("router dir") {
        sources.push(entry.expect("entry").path());
    }
    let mut routes = BTreeSet::new();
    for path in &sources {
        routes.extend(get_routes_in(
            &std::fs::read_to_string(path).expect("source"),
        ));
    }
    assert!(
        routes.len() > 50,
        "the parser found only {} GET routes",
        routes.len()
    );

    let listed: BTreeSet<String> = std::fs::read_to_string(root.join("tests/get_routes.txt"))
        .expect("tests/get_routes.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (path, note) = l.split_once('\t').expect("path<TAB>note");
            assert!(!note.trim().is_empty(), "{path}: say what it does");
            path.to_string()
        })
        .collect();

    let unreviewed: Vec<_> = routes.difference(&listed).collect();
    let gone: Vec<_> = listed.difference(&routes).collect();
    assert!(
        unreviewed.is_empty(),
        "GET routes nobody has reviewed for side effects: {unreviewed:?}. Check that each \
         changes nothing a cross-site request could abuse, then add it to tests/get_routes.txt"
    );
    assert!(
        gone.is_empty(),
        "listed GET routes that no longer exist: {gone:?}"
    );
}

#[test]
fn the_parser_sees_through_comments_and_method_chains() {
    let src = r#"
        .route(
            // a comment (with parentheses) before the path
            "/a",
            get(h::a).put(h::b),
        )
        .route("/b", post(h::c))
        .route("/c", any(h::d))
    "#;
    let found = get_routes_in(src);
    assert_eq!(found, ["/a", "/c"].iter().map(|s| s.to_string()).collect());
}

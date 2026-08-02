/// Route-surface lock (v2 point 3): the pg-core and fastcore MUST serve
/// the identical core-api contract. This asserts every PATH_* the pg-core
/// mounts is also mounted by fastcore, by parsing both `build_*_router`
/// source bodies. Any future route added to one core but not the other
/// fails this test — the cores can never silently diverge again.

/// Extract the `PATH_*` identifiers referenced inside the first
/// `fn <name>(` … balanced-brace body in `src`.
fn router_paths(src: &str, fn_marker: &str) -> std::collections::BTreeSet<String> {
    let start = src.find(fn_marker).expect("router fn present");
    let body = &src[start..];
    // take until the function's closing — good enough: scan to the next
    // top-level "\n}\n" after the fn (routers end with `.with_state`/merge).
    let end = body.find("\n}\n").map(|e| e + start).unwrap_or(src.len());
    let region = &src[start..end];
    let mut out = std::collections::BTreeSet::new();
    let bytes = region.as_bytes();
    let mut i = 0;
    while let Some(pos) = region[i..].find("PATH_") {
        let s = i + pos;
        let mut e = s;
        while e < bytes.len() && (bytes[e].is_ascii_alphanumeric() || bytes[e] == b'_') {
            e += 1;
        }
        out.insert(region[s..e].to_string());
        i = e;
    }
    out
}

#[test]
fn fastcore_serves_every_pg_core_route() {
    let pg_src = include_str!("mod.rs");
    let fc_src = include_str!("../../../fastcore/src/lib.rs");
    let pg = router_paths(pg_src, "fn build_full_router");
    let fc = router_paths(fc_src, "fn build_router");
    let missing: Vec<_> = pg.difference(&fc).cloned().collect();
    assert!(
        missing.is_empty(),
        "fastcore is missing pg-core contract routes (the two cores diverged): {missing:?}"
    );
}

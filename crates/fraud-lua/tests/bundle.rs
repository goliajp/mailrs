//! `scripts/deploy-fraud-rules.sh --activate` rebuilds the bundle from
//! develop's rule files, each followed by a newline in this order, and
//! refuses any other candidate. If `DEFAULT_SOURCE` were built otherwise,
//! the pack production runs and the pack the binaries carry would differ.
use mailrs_fraud_lua::DEFAULT_SOURCE;

#[test]
fn the_default_bundle_is_the_rule_files_each_followed_by_a_newline() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/rules");
    let mut bundle = String::new();
    for name in ["brands", "helpers", "identity", "provenance", "content"] {
        bundle.push_str(&std::fs::read_to_string(format!("{dir}/{name}.lua")).expect(name));
        bundle.push('\n');
    }
    assert!(bundle == DEFAULT_SOURCE);
    let files = std::fs::read_dir(dir).expect("rule directory").count();
    assert_eq!(files, 5, "a rule file the bundle leaves out");
}

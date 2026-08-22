//! Shared by the two request-contract files.
//!
//! Split out when `request_contract.rs` passed the 500-line limit. An
//! integration test file is its own crate, so the helpers both need
//! live here rather than being written twice — which is how two
//! fixtures come to be read with two slightly different paths.

pub fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../../wire-contract/requests/{}.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// Deserialize a fixture into `T`, failing with the serde error verbatim.
///
/// The error is the useful part: "missing field `sender`" names both the
/// struct's expectation and the client's omission in one line.
pub fn parse<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let raw = fixture(name);
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{name}.json does not fit the handler's struct: {e}"))
}

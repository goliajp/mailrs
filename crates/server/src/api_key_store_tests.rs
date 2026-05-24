//! Tests for `api_key_store` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;
use std::collections::HashSet;

#[test]
fn test_generate_key_format() {
    let (full_key, prefix, key_hash) = generate_api_key();

    // format: mlrs_{8hex}_{40hex} = 54 chars total
    assert_eq!(full_key.len(), 54, "key length should be 54, got {}", full_key.len());
    assert!(full_key.starts_with("mlrs_"), "key should start with mlrs_");
    assert_eq!(prefix.len(), 8, "prefix should be 8 hex chars");
    assert_eq!(key_hash.len(), 64, "sha256 hash should be 64 hex chars");

    // verify prefix is embedded in key
    assert!(full_key.starts_with(&format!("mlrs_{prefix}_")));

    // verify hash matches
    let expected_hash = sha256_hex(full_key.as_bytes());
    assert_eq!(key_hash, expected_hash);
}

#[test]
fn test_sha256_hex() {
    // known test vector: SHA-256 of empty string
    let hash = sha256_hex(b"");
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    // known test vector: SHA-256 of "hello"
    let hash = sha256_hex(b"hello");
    assert_eq!(
        hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn test_generate_key_uniqueness() {
    let mut prefixes = HashSet::new();
    let mut keys = HashSet::new();

    for _ in 0..100 {
        let (full_key, prefix, _) = generate_api_key();
        prefixes.insert(prefix);
        keys.insert(full_key);
    }

    assert_eq!(prefixes.len(), 100, "all 100 prefixes should be unique");
    assert_eq!(keys.len(), 100, "all 100 keys should be unique");
}

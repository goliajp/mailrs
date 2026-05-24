//! Tests for `valkey_store` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn validate_url_valid() {
    assert!(validate_url("redis://localhost:6379").is_ok());
    assert!(validate_url("redis://127.0.0.1:6379/0").is_ok());
}

#[test]
fn validate_url_invalid() {
    assert!(validate_url("not-a-url").is_err());
}

#[test]
fn validate_url_with_password() {
    assert!(validate_url("redis://:password@localhost:6379").is_ok());
}

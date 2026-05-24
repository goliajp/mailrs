//! Tests for `keys` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn validate_key_type_pgp() {
    assert!(validate_key_type("pgp"));
}

#[test]
fn validate_key_type_smime() {
    assert!(validate_key_type("smime"));
}

#[test]
fn validate_key_type_invalid() {
    assert!(!validate_key_type("rsa"));
    assert!(!validate_key_type(""));
    assert!(!validate_key_type("PGP"));
}

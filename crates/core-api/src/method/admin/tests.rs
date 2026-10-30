//! Serde round-trips over the admin wire types.

#![cfg(test)]

// These exercise types from several sibling modules, so they reach for
// the flat re-export rather than one file's contents.
use crate::method::admin::*;

#[test]
fn account_serde_with_hash_flatten() {
    let a = AccountWithHashWire {
        public: AccountWire {
            address: "u@x.com".into(),
            domain: "x.com".into(),
            display_name: "User".into(),
            active: true,
            created_at: 1_700_000_000,
            quota_bytes: 0,
            recovery_email: None,
            password_cleared_at: None,
            password_cleared_by: None,
        },
        password_hash: Some("$argon2id$...".into()),
    };
    let s = serde_json::to_string(&a).unwrap();
    // flatten should put `address` and `password_hash` at the top level
    assert!(s.contains("\"address\":\"u@x.com\""));
    assert!(s.contains("\"password_hash\""));
    let back: AccountWithHashWire = serde_json::from_str(&s).unwrap();
    assert_eq!(back.public.address, "u@x.com");
    assert!(back.password_hash.is_some());
}

#[test]
fn effective_permissions_roundtrip() {
    let p = EffectivePermissionsResponse {
        address: "admin@x.com".into(),
        permissions: vec!["admin.accounts".into(), "internal.rpc".into()],
        groups: vec![],
        is_super: true,
        send_as: vec!["billing@x.com".into()],
    };
    let s = serde_json::to_string(&p).unwrap();
    let back: EffectivePermissionsResponse = serde_json::from_str(&s).unwrap();
    assert_eq!(back.permissions.len(), 2);
    assert!(back.is_super);
    assert_eq!(back.send_as.len(), 1);
}

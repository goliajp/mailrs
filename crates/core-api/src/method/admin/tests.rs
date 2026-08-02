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
fn audit_default_limit_is_100() {
    let q: ListAuditQuery = serde_json::from_str("{}").unwrap();
    assert_eq!(q.limit, 100);
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

#[test]
fn api_key_omits_full_key_when_none() {
    let k = ApiKeyWire {
        id: 1,
        prefix: "mk_aBcD".into(),
        full_key: None,
        key_hash: "$argon2id$...".into(),
        account_address: "u@x.com".into(),
        name: "ci".into(),
        expires_at: None,
        last_used_at: None,
        revoked_at: None,
        created_at: 0,
        app_id: None,
    };
    let s = serde_json::to_string(&k).unwrap();
    assert!(!s.contains("full_key"));
    // key_hash is `#[serde(skip)]` so it must NOT leak
    assert!(!s.contains("key_hash"));
}

#[test]
fn export_request_omits_optional() {
    let r = ExportRequest {
        user: "u@x.com".into(),
        ..Default::default()
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(!s.contains("\"since\""));
    assert!(!s.contains("\"until\""));
    // Default::default() for u32 is 0 — but on the wire, when deserializing
    // a request that omits "limit", `#[serde(default = "default_export_limit")]`
    // gives us 1000. Verify the deser side:
    let r2: ExportRequest = serde_json::from_str(r#"{"user":"u@x.com"}"#).unwrap();
    assert_eq!(r2.limit, 1000);
}

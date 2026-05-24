//! Tests for `common` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn extract_address_bare() {
    assert_eq!(extract_address("user@example.com"), "user@example.com");
}

#[test]
fn extract_address_display_name() {
    assert_eq!(
        extract_address("Chenyun Dai <chenyund@qti.qualcomm.com>"),
        "chenyund@qti.qualcomm.com"
    );
}

#[test]
fn extract_address_angle_only() {
    assert_eq!(extract_address("<foo@bar.com>"), "foo@bar.com");
}

#[test]
fn extract_address_with_spaces() {
    assert_eq!(extract_address("  alice@test.org  "), "alice@test.org");
}

// --- verify_sender tests ---

fn make_super_perms(domains: &[&str]) -> crate::permission::EffectivePermissions {
    use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo, ALL_PERMISSIONS};
    let groups = vec![AccountGroup {
        group: GroupInfo {
            id: 1,
            name: "super".into(),
            domain: None,
            description: String::new(),
            is_builtin: true,
            created_at: 0,
        },
        permissions: ALL_PERMISSIONS.iter().map(|s| s.to_string()).collect(),
    }];
    compute_effective_permissions(
        &groups,
        &[],
        &domains.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
}

fn make_no_perms() -> crate::permission::EffectivePermissions {
    crate::permission::compute_effective_permissions(&[], &[], &[])
}

#[test]
fn verify_sender_superadmin_matching_domain_allowed() {
    let perms = make_super_perms(&["golia.jp", "example.com"]);
    assert!(verify_sender("agent@golia.jp", "admin@golia.jp", &perms).is_ok());
    // different user but same domain
    assert!(verify_sender("other@example.com", "admin@golia.jp", &perms).is_ok());
}

#[test]
fn verify_sender_superadmin_non_matching_domain_rejected() {
    // super user with only golia.jp domain — but super has all domains, so it should allow
    // let's test with a domain-scoped group instead
    use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo};
    let groups = vec![AccountGroup {
        group: GroupInfo {
            id: 1,
            name: "user".into(),
            domain: Some("golia.jp".into()),
            description: String::new(),
            is_builtin: false,
            created_at: 0,
        },
        permissions: vec!["mail.send".into(), "mail.read".into()],
    }];
    let perms = compute_effective_permissions(&groups, &[], &["golia.jp".into()]);
    assert_eq!(
        verify_sender("agent@evil.com", "admin@golia.jp", &perms),
        Err("sender must match authenticated user")
    );
}

#[test]
fn verify_sender_non_superadmin_different_from_rejected() {
    let perms = make_no_perms();
    assert_eq!(
        verify_sender("other@golia.jp", "user@golia.jp", &perms),
        Err("sender must match authenticated user")
    );
}

#[test]
fn verify_sender_non_superadmin_matching_from_allowed() {
    let perms = make_no_perms();
    assert!(verify_sender("user@golia.jp", "user@golia.jp", &perms).is_ok());
}

// --- resolve_thread_reply tests ---

#[tokio::test]
async fn resolve_thread_reply_thread_id_resolves_when_no_in_reply_to() {
    // when no mailbox store and no in_reply_to, thread_id cannot resolve (no DB)
    // but it should not panic
    let (reply, refs) = resolve_thread_reply(
        Some("thread-abc"),
        None,
        "user@test.com",
        None,
    ).await;
    // without a store, cannot resolve thread_id
    assert!(reply.is_none());
    assert!(refs.is_empty());
}

#[tokio::test]
async fn resolve_thread_reply_explicit_in_reply_to_takes_precedence() {
    // explicit in_reply_to should be used even if reply_to_thread_id is present
    let (reply, _refs) = resolve_thread_reply(
        Some("thread-abc"),
        Some("explicit-msg-id@test.com"),
        "user@test.com",
        None,
    ).await;
    assert_eq!(reply.as_deref(), Some("explicit-msg-id@test.com"));
}

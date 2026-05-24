//! Tests for `permission` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

fn make_group(name: &str, domain: Option<&str>, perms: &[&str]) -> AccountGroup {
    AccountGroup {
        group: GroupInfo {
            id: 1,
            name: name.to_string(),
            domain: domain.map(|s| s.to_string()),
            description: String::new(),
            is_builtin: false,
            created_at: 0,
        },
        permissions: perms.iter().map(|s| s.to_string()).collect(),
    }
}

fn all_domains() -> Vec<String> {
    vec!["golia.jp".into(), "golia.ai".into()]
}

#[test]
fn super_group_grants_everything() {
    let groups = vec![make_group("super", None, ALL_PERMISSIONS)];
    let perms = compute_effective_permissions(&groups, &[], &all_domains());

    assert!(perms.is_super());
    assert!(perms.has("mail.send"));
    assert!(perms.has("admin.groups"));
    assert!(perms.has("admin.impersonate"));
    assert_eq!(perms.accessible_domains().len(), 2);
}

#[test]
fn domain_user_group_basic_perms() {
    let groups = vec![make_group("user", Some("golia.jp"), &["mail.send", "mail.read"])];
    let perms = compute_effective_permissions(&groups, &[], &all_domains());

    assert!(!perms.is_super());
    assert!(perms.has("mail.send"));
    assert!(perms.has("mail.read"));
    assert!(!perms.has("admin.domains"));
    assert_eq!(perms.accessible_domains(), &["golia.jp"]);
}

#[test]
fn override_grants_extra_permission() {
    let groups = vec![make_group("user", Some("golia.jp"), &["mail.send", "mail.read"])];
    let overrides = vec![("admin.aliases".to_string(), true)];
    let perms = compute_effective_permissions(&groups, &overrides, &all_domains());

    assert!(perms.has("admin.aliases"));
    assert!(!perms.has("admin.domains"));
}

#[test]
fn override_revokes_group_permission() {
    let groups = vec![make_group("user", Some("golia.jp"), &["mail.send", "mail.read"])];
    let overrides = vec![("mail.send".to_string(), false)];
    let perms = compute_effective_permissions(&groups, &overrides, &all_domains());

    assert!(!perms.has("mail.send"));
    assert!(perms.has("mail.read"));
}

#[test]
fn super_with_revoke_override_downgrades() {
    let groups = vec![make_group("super", None, ALL_PERMISSIONS)];
    let overrides = vec![("admin.impersonate".to_string(), false)];
    let perms = compute_effective_permissions(&groups, &overrides, &all_domains());

    assert!(!perms.is_super());
    assert!(!perms.has("admin.impersonate"));
    assert!(perms.has("mail.send"));
}

#[test]
fn multiple_groups_union_permissions() {
    let groups = vec![
        make_group("user", Some("golia.jp"), &["mail.send", "mail.read"]),
        make_group("admin", Some("golia.ai"), &["admin.domains", "admin.accounts"]),
    ];
    let perms = compute_effective_permissions(&groups, &[], &all_domains());

    assert!(perms.has("mail.send"));
    assert!(perms.has("admin.domains"));
    let mut domains = perms.accessible_domains().to_vec();
    domains.sort();
    assert_eq!(domains, vec!["golia.ai", "golia.jp"]);
}

#[test]
fn no_groups_no_permissions() {
    let perms = compute_effective_permissions(&[], &[], &all_domains());

    assert!(!perms.is_super());
    assert!(!perms.has("mail.send"));
    assert!(perms.accessible_domains().is_empty());
}

#[test]
fn permission_list_sorted() {
    let groups = vec![make_group("user", Some("golia.jp"), &["mail.read", "mail.send"])];
    let perms = compute_effective_permissions(&groups, &[], &all_domains());
    let list = perms.permission_list();
    assert_eq!(list, vec!["mail.read", "mail.send"]);
}

#[test]
fn super_permission_list_complete() {
    let groups = vec![make_group("super", None, ALL_PERMISSIONS)];
    let perms = compute_effective_permissions(&groups, &[], &all_domains());
    let list = perms.permission_list();
    assert_eq!(list.len(), ALL_PERMISSIONS.len());
}

#[test]
fn has_any_admin_true_for_admin_perms() {
    let groups = vec![make_group("admin", Some("golia.jp"), &["admin.domains"])];
    let perms = compute_effective_permissions(&groups, &[], &all_domains());
    assert!(perms.has_any_admin());
}

#[test]
fn has_any_admin_false_for_mail_only() {
    let groups = vec![make_group("user", Some("golia.jp"), &["mail.send", "mail.read"])];
    let perms = compute_effective_permissions(&groups, &[], &all_domains());
    assert!(!perms.has_any_admin());
}

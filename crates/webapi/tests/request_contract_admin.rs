//! The bodies the admin console sends, against the structs the
//! handlers read.
//!
//! Split from `request_contract.rs` by who sends the request: this file
//! is the admin console, that one is the mail client. Both read the
//! same fixture directory and `every_fixture_has_a_test` over there
//! covers every file in it, so a fixture cannot land here unread.

mod common;
use common::parse;
use mailrs_webapi::handlers;

#[test]
fn greylist_add_body_matches() {
    let v: handlers::complete::CreateGreylistRequest = parse("greylist-local-add");
    assert_eq!(v.kind, "domain");
    assert_eq!(v.list, "blacklist");
    assert_eq!(v.value, "spam.example.com");
    assert_eq!(v.note, None);
}

#[test]
fn email_group_create_body_matches() {
    let v: handlers::groups::CreateEmailGroupRequest = parse("email-group-create");
    assert_eq!(v.address, "team@golia.jp");
    // The two that were silently dropped until 2026-07-30.
    assert_eq!(v.domain, "golia.jp");
    assert_eq!(v.description, "engineering");
}

#[test]
fn alias_create_body_matches() {
    let v: mailrs_core_api::method::admin::AddAliasRequest = parse("alias-create");
    assert_eq!(v.source_address, "devops@golia.jp");
    assert_eq!(v.target_address, "lihao@golia.jp");
    assert_eq!(v.domain, "golia.jp");
    assert_eq!(v.alias_type, "forward");
}

#[test]
fn account_create_body_matches() {
    let v: mailrs_core_api::method::admin::AddAccountRequest = parse("account-create");
    assert_eq!(v.address.as_str(), "qa@golia.jp");
    assert_eq!(v.display_name, "QA");
    assert_eq!(v.password, "not-a-real-password");
}

#[test]
fn domain_create_body_matches() {
    let v: mailrs_core_api::method::admin::AddDomainRequest = parse("domain-create");
    assert_eq!(v.name, "golia.jp");
}

#[test]
fn group_permissions_body_matches() {
    let v: handlers::groups::SetGroupPermissionsRequest = parse("group-permissions-set");
    assert_eq!(v.permissions, vec!["admin.accounts", "admin.aliases"]);
}

#[test]
fn credential_bodies_match() {
    let login: handlers::auth::LoginRequest = parse("login");
    assert_eq!(login.address, "lihao@golia.jp");
    assert_eq!(login.password, "not-a-real-password");
    // Absent unless the account has TOTP; present-and-absent must both parse.
    assert_eq!(login.totp_code, None);

    let change: handlers::auth::ChangePasswordRequest = parse("change-password");
    assert_eq!(change.current_password, "not-a-real-old-password");
    assert_eq!(change.new_password, "not-a-real-new-password");

    let reset: handlers::auth_recovery::ResetPasswordRequest = parse("reset-password");
    assert_eq!(reset.token, "0197f3c2-4a1b-7d31-9e55-2c8a1f0b6d44");
    assert_eq!(reset.new_password, "not-a-real-password");
}

#[test]
fn agent_key_create_body_matches() {
    let v: handlers::apps_keys::CreateAgentKeyRequest = parse("agent-key-create");
    assert_eq!(v.name, "ci-bot");
    assert_eq!(v.scopes, vec!["mail.read", "mail.send"]);
}

#[test]
fn remaining_admin_bodies_match() {
    let account: handlers::admin_directory::UpdateAccountRequest = parse("account-update");
    assert_eq!(account.display_name.as_deref(), Some("QA Team"));

    let group: handlers::groups::CreateGroupRequest = parse("group-create");
    assert_eq!(group.name, "admins");
    assert_eq!(group.description, "Full administrative access");

    let member: handlers::groups::AddGroupMemberRequest = parse("group-members-add");
    assert_eq!(member.address, "qa@golia.jp");

    // The email-group membership body has the same one field and its own
    // handler, so it gets its own fixture rather than sharing one — two
    // paths that happen to agree today are not one contract.
    let eg_member: handlers::groups::AddGroupMemberRequest = parse("email-group-members-add");
    assert_eq!(eg_member.address, "qa@golia.jp");
}

/// TOTP enable and disable send the same one-field body to two handlers.
///
#[test]
fn account_sieve_body_matches() {
    let v: handlers::admin_ops::SetSieveRequest = parse("account-sieve-set");
    assert!(v.script.starts_with("require [\"fileinto\"];\n"));
    assert!(v.script.contains("fileinto \"Notifications\";"));
    assert!(v.script.ends_with('\n'), "the trailing newline survives");
}

#[test]
fn system_config_body_matches() {
    let v: handlers::system_config::SetSystemConfigRequest = parse("system-config-set");
    assert_eq!(v.value, "mailrs");
}

/// Removing a sign-in method.
///
#[test]
fn identity_unlink_body_matches() {
    let v: handlers::external_login::UnlinkRequest = parse("identity-unlink");
    assert_eq!(v.issuer, "https://accounts.google.com");
    assert_eq!(v.subject, "1029384756");
}

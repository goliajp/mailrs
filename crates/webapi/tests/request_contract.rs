//! Every fixture in `wire-contract/requests/` must deserialize into the
//! struct the handler actually reads.
//!
//! The fixtures are the bodies the web client sends. This side checks the
//! backend against them; `web/src/wire/__tests__/request-contract.test.ts`
//! checks the client against the same files. Change one side without the
//! other and one of the two goes red.
//!
//! Why this exists: an audit on 2026-07-30 found nine of thirty-five
//! request bodies wrong — four failing every call, five succeeding while
//! dropping the value the user had supplied. Tests existed. `api.test.ts`
//! asserted the snooze body was `{until: <ISO>}` and passed on every run
//! while every snooze in production answered 422, because it pinned what
//! the frontend had decided to send. A test that checks one side against
//! itself proves nothing about the other.
//!
//! Read at runtime rather than `include_str!` so the crate stays packageable
//! — the files live above `CARGO_MANIFEST_DIR`.

use mailrs_webapi::handlers;

fn fixture(name: &str) -> String {
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
fn parse<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let raw = fixture(name);
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{name}.json does not fit the handler's struct: {e}"))
}

#[test]
fn snooze_body_matches() {
    let v: handlers::conversations::SnoozeBody = parse("snooze");
    assert_eq!(v.snoozed_until, 1_785_542_400);
}

#[test]
fn feedback_body_matches() {
    let v: handlers::prefs_misc::FeedbackRequest = parse("feedback");
    assert_eq!(v.sender_email, "someone@example.com");
    assert_eq!(v.action, "block");
}

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
    let v: handlers::complete::CreateEmailGroupRequest = parse("email-group-create");
    assert_eq!(v.address, "team@golia.jp");
    // The two that were silently dropped until 2026-07-30.
    assert_eq!(v.domain, "golia.jp");
    assert_eq!(v.description, "engineering");
}

#[test]
fn signature_save_body_matches() {
    let v: mailrs_core_api::method::admin::SaveSignatureRequest = parse("signature-save");
    assert_eq!(v.name, "default");
    // `html`, not `html_content`. The client sent the latter into a
    // defaulted field, so every signature saved with an empty body.
    assert_eq!(v.html, "<p>Regards</p>");
}

#[test]
fn key_upload_body_matches() {
    let v: handlers::keys::SetKeyRequest = parse("key-upload");
    assert!(v.public_key.contains("PGP PUBLIC KEY"));
    // Not sent, and defaulted rather than required: neither backend derives
    // a fingerprint, so the client does not claim one.
    assert_eq!(v.fingerprint, "");
}

#[test]
fn webhook_create_body_matches() {
    let v: handlers::complete::CreateAgentWebhookRequest = parse("webhook-create");
    assert_eq!(v.url, "https://hooks.example.com/mailrs");
    // Dropped until 2026-07-30, which stored a webhook scoped to one
    // sender as one matching everything.
    assert_eq!(v.filter_sender.as_deref(), Some("alerts@example.com"));
    assert_eq!(v.filter_thread_id, None);
}

#[test]
fn calendar_feed_create_body_matches() {
    let v: handlers::calendar::CreateFeedRequest = parse("calendar-feed-create");
    assert_eq!(v.name, "Team calendar");
    assert_eq!(v.url, "https://cal.example.com/team.ics");
    // Removed from the form on 2026-07-30 because no fetcher consumed them,
    // restored with the one that does. The handler dropping either again
    // means a feed behind basic auth silently 401s forever.
    assert_eq!(v.basic_auth_user.as_deref(), Some("team"));
    assert_eq!(v.basic_auth_pass.as_deref(), Some("hunter2"));
}

#[test]
fn send_body_matches() {
    let v: handlers::compose::SendRequest = parse("send");
    assert_eq!(v.to, vec!["someone@example.com".to_string()]);
    // Epoch seconds. An ISO string here fails this test, which is what the
    // client sent until 2.19.2 — 422 on this path, and on the multipart
    // path a silent immediate send.
    assert_eq!(v.scheduled_at, Some(1_785_542_400));
}

#[test]
fn send_redraft_body_matches() {
    let v: handlers::compose::SendRequest = parse("send-redraft");
    assert_eq!(v.redraft_of.as_deref(), Some("abc123@golia.jp"));
    // `Some(vec![...])`, not a flattened list: absent keeps every carried
    // attachment and present-and-empty keeps none, so the Option must
    // survive the round trip.
    assert_eq!(v.redraft_keep, Some(vec![0, 2]));
}

#[test]
fn forgot_password_body_matches() {
    let v: handlers::complete::ForgotPasswordRequest = parse("forgot-password");
    assert_eq!(v.address, "lihao@golia.jp");
    // Dropped until 2026-07-30, which meant the claimed recovery address
    // was never verified.
    assert_eq!(v.recovery_email, "backup@example.com");
}

#[test]
fn batch_mutation_body_matches() {
    let v: handlers::conversations::BatchRequest = parse("batch-mutation");
    assert_eq!(v.action, "archive");
    assert_eq!(v.thread_ids.len(), 2);
}

/// The three writing-assistance bodies.
///
/// `reply-suggest` is the reason these exist. Its three `original_*` fields
/// are required, and the client sent `sender` and `subject` instead — serde
/// dropped both, `original_sender` was then missing, and every call was a
/// 422. The button had never worked on the lane that had the route, and the
/// lane production runs had no route at all.
#[test]
fn ai_bodies_match() {
    let polish: mailrs_intelligence::assist::PolishRequest = parse("ai-polish");
    assert_eq!(polish.text, "please make this better");
    assert_eq!(polish.tone, "professional");

    let suggest: mailrs_intelligence::assist::ReplySuggestRequest = parse("ai-reply-suggest");
    assert_eq!(suggest.original_sender, "nagata@nagatax.tokyo.jp");
    assert_eq!(suggest.original_subject, "Meeting");
    assert_eq!(suggest.original_body, "Are you free on Thursday?");
    // Not sent by the client; the default is what the handler uses.
    assert_eq!(suggest.tone, "professional");

    let subject: mailrs_intelligence::assist::SubjectGenerateRequest = parse("ai-generate-subject");
    assert_eq!(subject.body, "Confirming Thursday at 3pm.");
    assert_eq!(
        subject.context.as_deref(),
        Some("To: nagata@nagatax.tokyo.jp")
    );
}

/// The draft autosave, which runs every three seconds while composing.
///
/// The client sent an untyped `Record<string, unknown>` here, so a renamed
/// field compiled and serde dropped it. `id` is the field that matters
/// most: absent allocates a new draft, present upserts the same one, so
/// losing it turns one draft into one per tick.
#[test]
fn draft_save_body_matches() {
    let v: mailrs_core_api::method::admin::SaveDraftRequest = parse("draft-save");
    assert_eq!(v.id, Some(42));
    assert_eq!(v.to, "nagata@nagatax.tokyo.jp");
    assert_eq!(v.cc, "someone@example.com");
    assert_eq!(v.subject, "Re: Meeting");
    assert_eq!(v.body, "Confirming Thursday at 3pm.");
    // Reopening a reply from the Draft tab lost this until 2026-07-30.
    assert_eq!(
        v.reply_to_thread_id.as_deref(),
        Some("a48529b44b1b190f@golia.jp")
    );
}

/// Alias creation — the admin write with the worst failure mode.
///
/// Every non-account address on these domains resolves through an alias, so
/// a dropped field here is mail that goes nowhere. All four are required on
/// the handler; the admin page sends exactly these and nothing checked that
/// until now.
#[test]
fn alias_create_body_matches() {
    let v: mailrs_core_api::method::admin::AddAliasRequest = parse("alias-create");
    assert_eq!(v.source_address, "devops@golia.jp");
    assert_eq!(v.target_address, "lihao@golia.jp");
    assert_eq!(v.domain, "golia.jp");
    assert_eq!(v.alias_type, "forward");
}

/// Account provisioning. A dropped field here is an account that cannot
/// log in — `password` is hashed server-side, so losing it stores an
/// account with no usable credential rather than failing.
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

/// Saving a group's permissions. This was a 405 in production until
/// 2026-07-31 because the lane registered POST while the page sends PUT,
/// so the body had never reached the handler to be checked.
#[test]
fn group_permissions_body_matches() {
    let v: handlers::complete::SetGroupPermissionsRequest = parse("group-permissions-set");
    assert_eq!(v.permissions, vec!["admin.accounts", "admin.aliases"]);
}

/// The credential bodies. A fixture with a fake password is worth having:
/// these are the paths where a dropped field means nobody can log in, and
/// "it sends a secret" is a reason to write the fixture carefully, not a
/// reason to leave the shape unchecked.
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

    let reset: handlers::complete::ResetPasswordRequest = parse("reset-password");
    assert_eq!(reset.token, "0197f3c2-4a1b-7d31-9e55-2c8a1f0b6d44");
    assert_eq!(reset.new_password, "not-a-real-password");
}

/// Recovery email — one of the nine wrong on 2026-07-30, where a new
/// account's setting threw.
#[test]
fn recovery_email_body_matches() {
    let v: handlers::complete::SetRecoveryEmailRequest = parse("recovery-email-set");
    assert_eq!(v.recovery_email.as_deref(), Some("backup@example.com"));
}

/// An agent key's scopes decide what a machine caller may do. Dropping the
/// list would create a key with none, or with whatever the handler defaults
/// to — neither is what the operator asked for.
#[test]
fn agent_key_create_body_matches() {
    let v: handlers::complete::CreateAgentKeyRequest = parse("agent-key-create");
    assert_eq!(v.name, "ci-bot");
    assert_eq!(v.scopes, vec!["mail.read", "mail.send"]);
}

/// Reactions are keyed by the emoji itself, so it has to survive the round
/// trip as typed — a multi-byte character that arrives mangled is a
/// reaction nobody can remove, because removing it sends the same string.
#[test]
fn reaction_toggle_body_matches() {
    let v: mailrs_core_api::method::admin::ToggleReactionRequest = parse("reaction-toggle");
    assert_eq!(v.emoji, "\u{1f44d}");
}

/// The remaining admin writes. Each is small, and each drops silently:
/// serde ignores what it does not name, so a renamed field leaves the
/// operator looking at a form that said it saved.
#[test]
fn remaining_admin_bodies_match() {
    let account: handlers::admin::UpdateAccountRequest = parse("account-update");
    assert_eq!(account.display_name.as_deref(), Some("QA Team"));

    let group: handlers::complete::CreateGroupRequest = parse("group-create");
    assert_eq!(group.name, "admins");
    assert_eq!(group.description, "Full administrative access");

    let member: handlers::complete::AddGroupMemberRequest = parse("group-members-add");
    assert_eq!(member.address, "qa@golia.jp");

    // The email-group membership body has the same one field and its own
    // handler, so it gets its own fixture rather than sharing one — two
    // paths that happen to agree today are not one contract.
    let eg_member: handlers::complete::AddGroupMemberRequest = parse("email-group-members-add");
    assert_eq!(eg_member.address, "qa@golia.jp");
}

/// TOTP enable and disable send the same one-field body to two handlers.
///
/// One fixture, because it is one shape — but both call sites are asserted
/// on the client side, since two paths agreeing today is not one contract.
#[test]
fn totp_code_body_matches() {
    let v: handlers::complete::TotpCodeRequest = parse("totp-code");
    assert_eq!(v.code, "123456");
}

/// A sieve script is submitted whole and whitespace is significant — the
/// rules are line-oriented, so a body that arrives reflowed is a filter
/// that no longer parses.
#[test]
fn account_sieve_body_matches() {
    let v: handlers::admin::SetSieveRequest = parse("account-sieve-set");
    assert!(v.script.starts_with("require [\"fileinto\"];\n"));
    assert!(v.script.contains("fileinto \"Notifications\";"));
    assert!(v.script.ends_with('\n'), "the trailing newline survives");
}

/// `{value}`, not a bare string. The handler took a `serde_json::Value` and
/// stored `body.as_str()` with the whole document's JSON text as its
/// fallback, so every setting would have been stored as the literal
/// `{"value":"..."}` — never seen because the route took POST while the page
/// sends PUT and the request was a 405.
#[test]
fn system_config_body_matches() {
    let v: handlers::complete::SetSystemConfigRequest = parse("system-config-set");
    assert_eq!(v.value, "mailrs");
}

/// Removing a sign-in method.
///
/// The body names the identity; the account comes from the session and never
/// from here. Both ends state that rule — the handler takes the address from
/// `AuthedUser` and `unlink` refuses when the link belongs to someone else —
/// because it is the one that keeps a link from being detached by whoever
/// can guess it.
#[test]
fn identity_unlink_body_matches() {
    let v: handlers::external_login::UnlinkRequest = parse("identity-unlink");
    assert_eq!(v.issuer, "https://accounts.google.com");
    assert_eq!(v.subject, "1029384756");
}

/// An unknown field is refused, by name.
///
/// The point of `deny_unknown_fields` is that the failure says which field.
/// Without it serde drops what it does not recognise and the request
/// succeeds having ignored part of what the user asked for — nine bodies
/// were doing exactly that on 2026-07-30, five of them silently.
#[test]
fn an_unknown_field_is_named_rather_than_dropped() {
    let mut body: serde_json::Value =
        serde_json::from_str(&fixture("draft-save")).expect("fixture parses");
    body["replyToThreadId"] = serde_json::json!("camelCased by mistake");

    let err = serde_json::from_value::<mailrs_core_api::method::admin::SaveDraftRequest>(body)
        .expect_err("an unknown field must not deserialize");
    let msg = err.to_string();
    assert!(
        msg.contains("replyToThreadId"),
        "the error must name the field so it can be fixed; got: {msg}"
    );

    // And the correct spelling still works, so this is a gate and not a wall.
    let ok: mailrs_core_api::method::admin::SaveDraftRequest = parse("draft-save");
    assert_eq!(
        ok.reply_to_thread_id.as_deref(),
        Some("a48529b44b1b190f@golia.jp")
    );
}

/// Every fixture is checked by a test above.
///
/// Without this, adding a fixture and forgetting the case leaves the file
/// sitting in the directory looking like coverage it is not providing —
/// which is the same shape of problem as a test that checks one side
/// against itself.
#[test]
fn every_fixture_has_a_test() {
    const CHECKED: &[&str] = &[
        "ai-generate-subject",
        "account-create",
        "account-sieve-set",
        "account-update",
        "agent-key-create",
        "alias-create",
        "domain-create",
        "group-permissions-set",
        "ai-polish",
        "ai-reply-suggest",
        "batch-mutation",
        "change-password",
        "calendar-feed-create",
        "draft-save",
        "email-group-create",
        "email-group-members-add",
        "feedback",
        "forgot-password",
        "greylist-local-add",
        "identity-unlink",
        "group-create",
        "group-members-add",
        "login",
        "key-upload",
        "reaction-toggle",
        "recovery-email-set",
        "reset-password",
        "send",
        "send-redraft",
        "signature-save",
        "snooze",
        "system-config-set",
        "totp-code",
        "webhook-create",
    ];
    let dir = format!(
        "{}/../../wire-contract/requests",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {dir}: {e}"))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".json").map(str::to_string)
        })
        .collect();
    found.sort();

    let mut expected: Vec<String> = CHECKED.iter().map(|s| s.to_string()).collect();
    expected.sort();

    assert_eq!(
        found, expected,
        "a fixture was added or removed without updating this file — every \
         entry needs a case here and one in \
         web/src/wire/__tests__/request-contract.test.ts"
    );
}

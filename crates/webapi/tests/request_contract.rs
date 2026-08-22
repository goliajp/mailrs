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

mod common;
use common::{fixture, parse};
use mailrs_webapi::handlers;

#[test]
fn push_register_body_matches() {
    let v: handlers::push::RegisterPushTokenRequest = parse("push-register");
    assert!(!v.token.is_empty());
    assert_eq!(v.platform, "ios");
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
fn signature_save_body_matches() {
    let v: mailrs_core_api::method::admin::SaveSignatureRequest = parse("signature-save");
    assert_eq!(v.name, "default");
    // `html`, not `html_content`. The client sent the latter into a
    // defaulted field, so every signature saved with an empty body.
    assert_eq!(v.html, "<p>Regards</p>");
}

/// The address the client sends is the field the handler reads.
///
#[test]
fn sender_list_add_body_matches() {
    let v: handlers::spam_lists::AddRequest = parse("sender-list-add");
    assert_eq!(v.address, "friend@example.com");
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
    let v: handlers::apps_keys::CreateAgentWebhookRequest = parse("webhook-create");
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
    let v: handlers::auth_recovery::ForgotPasswordRequest = parse("forgot-password");
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
#[test]
fn recovery_email_body_matches() {
    let v: handlers::auth_recovery::SetRecoveryEmailRequest = parse("recovery-email-set");
    assert_eq!(v.recovery_email.as_deref(), Some("backup@example.com"));
}

#[test]
fn reaction_toggle_body_matches() {
    let v: mailrs_core_api::method::admin::ToggleReactionRequest = parse("reaction-toggle");
    assert_eq!(v.emoji, "\u{1f44d}");
}

#[test]
fn totp_code_body_matches() {
    let v: handlers::auth_recovery::TotpCodeRequest = parse("totp-code");
    assert_eq!(v.code, "123456");
}

/// An unknown field is refused, by name.
///
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
/// The body names a message, never a URL.
///
#[test]
fn unsubscribe_body_matches() {
    let v: handlers::unsubscribe::UnsubscribeRequest = parse("unsubscribe");
    assert!(!v.thread_id.is_empty());
    assert_eq!(v.uid, 41);
}

/// The body a set-up screen posts to connect a mailbox somewhere else.
///
#[test]
fn external_account_create() {
    let v: serde_json::Value = parse("external-account-create");
    assert_eq!(v["email"], "someone@qq.com");
    assert!(v["secret"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(
        v.get("incoming").is_none(),
        "the client should not need to know the host"
    );
    assert!(v.get("outgoing").is_none());
}

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
        "external-account-create",
        "group-permissions-set",
        "push-register",
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
        "sender-list-add",
        "signature-save",
        "snooze",
        "system-config-set",
        "unsubscribe",
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

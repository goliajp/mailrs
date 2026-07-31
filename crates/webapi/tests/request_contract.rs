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
    let v: handlers::prefs::FeedbackRequest = parse("feedback");
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
    let v: handlers::prefs::SendRequest = parse("send");
    assert_eq!(v.to, vec!["someone@example.com".to_string()]);
    // Epoch seconds. An ISO string here fails this test, which is what the
    // client sent until 2.19.2 — 422 on this path, and on the multipart
    // path a silent immediate send.
    assert_eq!(v.scheduled_at, Some(1_785_542_400));
}

#[test]
fn send_redraft_body_matches() {
    let v: handlers::prefs::SendRequest = parse("send-redraft");
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
        "ai-polish",
        "ai-reply-suggest",
        "batch-mutation",
        "calendar-feed-create",
        "email-group-create",
        "feedback",
        "forgot-password",
        "greylist-local-add",
        "key-upload",
        "send",
        "send-redraft",
        "signature-save",
        "snooze",
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

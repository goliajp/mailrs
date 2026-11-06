//! Every fixture in `wire-contract/requests/` is read by a test on this
//! side: `request_contract.rs` and the `request_contract_*.rs` files beside
//! it. A fixture nobody reads is not coverage.

#[test]
fn every_fixture_has_a_test() {
    const CHECKED: &[&str] = &[
        "password-clear",
        "password-reset-with-code",
        "ai-generate-subject",
        "account-create",
        "account-forwards-set",
        "account-password-set",
        "outbound-resume",
        "account-password-set-compromised",
        "account-send-as-set",
        "account-sieve-set",
        "account-update",
        "agent-key-create",
        "alias-create",
        "domain-create",
        "group-permissions-set",
        "push-register",
        "ai-polish",
        "ai-reply-suggest",
        "batch-mutation",
        "change-password",
        "calendar-feed-create",
        "draft-save",
        "draft-save-attachments",
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
        "label-create",
        "label-update",
        "senders-accept",
        "masked-email-create",
        "masked-email-update",
        "simplelogin-alias-new",
        "addy-alias-create",
        "forwarding-request",
        "forwarding-verify",
        "rule-create",
        "rule-preview",
        "saved-search-create",
        "conversation-search",
        "conversation-mark-all-read",
        "screening-hash",
        "screening-observation",
        "app-password-create",
        "protocol-password-set",
        "mock-address-create",
        "mock-namespace-create",
        "invite-counter",
        "imap-import-start",
        "invite-rsvp",
        "mock-send",
        "mock-settings-save",
        "mock-webhook-set",
        "recovery-email-set",
        "reset-password",
        "send",
        "send-team-reply",
        "send-redraft",
        "shared-assign",
        "shared-note",
        "shared-presence",
        "shared-draft-announce",
        "email-group-shared-set",
        "send-undo",
        "sender-list-add",
        // read in request_contract_admin.rs
        "ops-silence-set",
        "ops-silence-end",
        "ops-maintenance-start",
        // read in request_contract_block.rs
        "block-sender",
        "unblock-sender",
        "signature-save",
        "snooze",
        "system-config-set",
        "template-save",
        "unsubscribe",
        "vacation-set",
        // read in request_contract_swipe.rs
        "swipe-set",
        "totp-code",
        "confirm",
        "webhook-create",
        "notify-prefs-set",
        "notify-override",
        "important-set",
        "sender-reclassify",
        "sender-rule-set",
        "web-push-subscribe",
        "web-push-unsubscribe",
        "unified-push-register",
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

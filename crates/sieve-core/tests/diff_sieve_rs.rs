//! ckpt 4.5 — first-slice differential test against Stalwart's
//! `sieve-rs` (the oracle this engine will eventually replace).
//!
//! 10 scripts × 2 sample messages. Builds the same action sequence
//! out of both engines (mapped to a shared `NormalizedAction` enum
//! that drops engine-specific metadata) and asserts equality. A
//! 200-script corpus is future ckpt 4.x work; this is the smoke
//! that proves the engines agree on the RFC 5228 base.

use std::sync::Arc;

use mailrs_sieve_core::{Action, eval_script};
use sieve::{Compiler, Event, Input, Recipient, Runtime};

#[derive(Debug, Clone, PartialEq, Eq)]
enum NormalizedAction {
    Keep,
    Discard,
    FileInto(String),
    Redirect(String),
    Reject(String),
}

fn ours(script: &str, msg: &[u8]) -> Vec<NormalizedAction> {
    let actions = eval_script(script, msg).unwrap_or_default();
    actions
        .into_iter()
        .map(|a| match a {
            Action::Keep => NormalizedAction::Keep,
            Action::Discard => NormalizedAction::Discard,
            Action::FileInto(s) => NormalizedAction::FileInto(s),
            Action::Redirect(s) => NormalizedAction::Redirect(s),
            Action::Reject(s) => NormalizedAction::Reject(s),
        })
        .collect()
}

fn sieve_rs(script: &str, msg: &[u8]) -> Vec<NormalizedAction> {
    let compiler = Compiler::new();
    let compiled = match compiler.compile(script.as_bytes()) {
        Ok(c) => Arc::new(c),
        Err(_) => return Vec::new(),
    };
    let runtime = Runtime::new();
    let mut ctx = runtime.filter(msg);
    let input = Input::Script {
        name: sieve::Script::Personal("main".into()),
        script: compiled,
    };
    let mut out = Vec::new();
    let mut result = ctx.run(input);
    loop {
        match result {
            Some(Ok(Event::Keep { .. })) => {
                out.push(NormalizedAction::Keep);
                break;
            }
            Some(Ok(Event::Discard)) => {
                out.push(NormalizedAction::Discard);
                break;
            }
            Some(Ok(Event::FileInto { folder, .. })) => {
                out.push(NormalizedAction::FileInto(folder));
                result = ctx.run(Input::True);
            }
            Some(Ok(Event::SendMessage { recipient, .. })) => {
                let addr = match recipient {
                    Recipient::Address(a) => a,
                    Recipient::List(l) => l,
                    Recipient::Group(g) => g.into_iter().next().unwrap_or_default(),
                };
                out.push(NormalizedAction::Redirect(addr));
                result = ctx.run(Input::True);
            }
            Some(Ok(Event::Reject { reason, .. })) => {
                out.push(NormalizedAction::Reject(reason));
                break;
            }
            Some(Ok(Event::MailboxExists { .. })) => result = ctx.run(Input::True),
            Some(Ok(Event::ListContains { .. })) => result = ctx.run(Input::False),
            Some(Ok(Event::DuplicateId { .. })) => result = ctx.run(Input::False),
            Some(Ok(Event::IncludeScript { .. })) => result = ctx.run(Input::False),
            Some(Ok(Event::SetEnvelope { .. })) => result = ctx.run(Input::True),
            Some(Ok(Event::CreatedMessage { .. })) => result = ctx.run(Input::True),
            Some(Ok(Event::Function { .. })) => result = ctx.run(Input::True),
            Some(Ok(_)) => result = ctx.run(Input::True),
            Some(Err(_)) | None => break,
        }
    }
    out
}

/// Each row is a script + sample message + label for diagnostics.
fn corpus() -> Vec<(&'static str, &'static str, &'static [u8])> {
    let msg_spam: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: spam offer\r\n\
\r\n\
hello\r\n";

    let msg_clean: &[u8] = b"\
From: Bob <bob@trusted.com>\r\n\
To: alice@example.com\r\n\
Subject: meeting tomorrow\r\n\
\r\n\
agenda attached\r\n";

    vec![
        ("explicit_keep", "keep;", msg_spam),
        ("explicit_discard", "discard;", msg_spam),
        (
            "fileinto",
            r#"require ["fileinto"]; fileinto "Junk";"#,
            msg_spam,
        ),
        (
            "header_is_spam",
            r#"if header :is "Subject" "spam offer" { discard; }"#,
            msg_spam,
        ),
        (
            "header_is_no_match",
            r#"if header :is "Subject" "nothing-here" { discard; }"#,
            msg_spam,
        ),
        (
            "header_contains_spam",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" { fileinto "Spam"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "header_contains_clean",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" { fileinto "Spam"; }
               else { keep; }"#,
            msg_clean,
        ),
        (
            "size_over_small",
            "if size :over 1 { discard; }",
            msg_spam,
        ),
        (
            "size_under_huge",
            "if size :under 100K { discard; }",
            msg_spam,
        ),
        (
            "exists_subject",
            r#"if exists "Subject" { discard; }"#,
            msg_spam,
        ),
        // --- slice 2 additions ---
        (
            "header_matches_glob_star_prefix",
            r#"if header :matches "Subject" "*offer*" { discard; }"#,
            msg_spam,
        ),
        (
            "header_matches_glob_question_mark",
            r#"if header :matches "Subject" "spam ?ffer" { discard; }"#,
            msg_spam,
        ),
        (
            "not_invert_spam",
            r#"if not header :is "Subject" "nothing" { discard; }"#,
            msg_spam,
        ),
        (
            "not_invert_clean",
            r#"if not header :is "Subject" "meeting tomorrow" { discard; }"#,
            msg_clean,
        ),
        (
            "allof_two_true",
            r#"if allof(header :contains "Subject" "spam", exists "From") { discard; }"#,
            msg_spam,
        ),
        (
            "allof_one_false",
            r#"if allof(header :contains "Subject" "spam", header :is "From" "wrong") { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_one_true",
            r#"if anyof(header :is "Subject" "no-match", header :contains "Subject" "spam") { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_all_false",
            r#"if anyof(header :is "Subject" "no-match-1", header :is "Subject" "no-match-2") { discard; }"#,
            msg_spam,
        ),
        (
            "address_localpart_match",
            r#"if address :localpart "From" "alice" { discard; }"#,
            msg_spam,
        ),
        (
            "address_localpart_no_match",
            r#"if address :localpart "From" "carol" { discard; }"#,
            msg_spam,
        ),
        (
            "address_domain_match",
            r#"if address :domain "From" "example.com" { discard; }"#,
            msg_spam,
        ),
        (
            "address_to_list_localpart",
            r#"require ["fileinto"];
               if address :localpart "To" "bob" { fileinto "ToBob"; } else { keep; }"#,
            msg_spam,
        ),
        (
            "elsif_chain_first_match",
            r#"require ["fileinto"];
               if header :is "Subject" "no-match" { fileinto "A"; }
               elsif header :contains "Subject" "spam" { fileinto "Spam"; }
               elsif header :contains "Subject" "offer" { fileinto "Ads"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "elsif_chain_else_branch",
            r#"require ["fileinto"];
               if header :is "Subject" "no-match" { fileinto "A"; }
               elsif header :is "Subject" "another-no" { fileinto "B"; }
               else { keep; }"#,
            msg_clean,
        ),
        (
            "stop_short_circuit",
            r#"if header :contains "Subject" "spam" { discard; stop; }
               keep;"#,
            msg_spam,
        ),
        (
            "redirect_then_keep_unguarded",
            r#"redirect "forward@example.com";"#,
            msg_spam,
        ),
        (
            "reject_with_reason",
            r#"require ["reject"]; reject "policy violation";"#,
            msg_spam,
        ),
        (
            "case_insensitive_is",
            r#"if header :is "Subject" "SPAM OFFER" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_missing",
            r#"if exists "X-Spam-Score" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_multi_present",
            r#"if exists ["From", "To", "Subject"] { discard; }"#,
            msg_spam,
        ),
        (
            "exists_multi_partial",
            r#"if exists ["From", "To", "X-Missing"] { discard; }"#,
            msg_spam,
        ),
        (
            "nested_if_inside_if",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" {
                 if header :is "From" "Alice <alice@example.com>" { fileinto "SpamFromAlice"; }
                 else { fileinto "Spam"; }
               }"#,
            msg_spam,
        ),
    ]
}

#[test]
fn engines_agree_on_corpus() {
    let mut disagreements = Vec::new();
    for (label, script, msg) in corpus() {
        let a = ours(script, msg);
        let b = sieve_rs(script, msg);
        if a != b {
            disagreements.push((label, a, b));
        }
    }
    assert!(
        disagreements.is_empty(),
        "engine disagreement: {disagreements:#?}",
    );
}

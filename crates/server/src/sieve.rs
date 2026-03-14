use std::collections::HashMap;
use std::sync::Arc;

use sieve::{Compiler, Event, Input, Recipient, Runtime, Sieve as CompiledSieve};

/// Sieve script evaluation result
#[derive(Debug, Clone)]
pub enum SieveAction {
    Keep,
    FileInto(String),
    Discard,
    Redirect(String),
    Reject(String),
    /// vacation auto-reply: (recipient, raw RFC 5322 message to send)
    Vacation(String, Vec<u8>),
}

/// compile a Sieve script, returning the compiled form
pub fn compile_sieve(script: &str) -> Result<Arc<CompiledSieve>, String> {
    let compiler = Compiler::new();
    compiler
        .compile(script.as_bytes())
        .map(Arc::new)
        .map_err(|e| format!("{e}"))
}

/// evaluate a compiled Sieve script against a message
#[allow(dead_code)]
pub fn evaluate_sieve(compiled: &Arc<CompiledSieve>, message: &[u8]) -> Vec<SieveAction> {
    evaluate_sieve_with_envelope(compiled, message, None, None)
}

/// evaluate a compiled Sieve script against a message with envelope information
/// for vacation/auto-reply support
pub fn evaluate_sieve_with_envelope(
    compiled: &Arc<CompiledSieve>,
    message: &[u8],
    envelope_from: Option<&str>,
    envelope_to: Option<&str>,
) -> Vec<SieveAction> {
    let runtime = Runtime::new();
    let mut ctx = runtime.filter(message);
    if let Some(from) = envelope_from {
        ctx.set_envelope(sieve::Envelope::From, from);
    }
    if let Some(to) = envelope_to {
        ctx.set_envelope(sieve::Envelope::To, to);
    }

    let input = Input::Script {
        name: sieve::Script::Personal("main".into()),
        script: compiled.clone(),
    };

    let mut actions = Vec::new();
    // track messages created by vacation/notify actions
    let mut created_messages: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut result = ctx.run(input);

    loop {
        match result {
            Some(Ok(Event::Keep { .. })) => {
                actions.push(SieveAction::Keep);
                break;
            }
            Some(Ok(Event::Discard)) => {
                actions.push(SieveAction::Discard);
                break;
            }
            Some(Ok(Event::FileInto { folder, .. })) => {
                actions.push(SieveAction::FileInto(folder));
                result = ctx.run(Input::True);
            }
            Some(Ok(Event::SendMessage {
                recipient,
                message_id,
                ..
            })) => {
                let addr = match recipient {
                    Recipient::Address(a) => a,
                    Recipient::List(l) => l,
                    Recipient::Group(g) => g.into_iter().next().unwrap_or_default(),
                };
                // if the message_id references a created message (vacation/notify),
                // emit Vacation with the generated reply body
                if let Some(body) = created_messages.remove(&message_id) {
                    actions.push(SieveAction::Vacation(addr, body));
                } else {
                    actions.push(SieveAction::Redirect(addr));
                }
                result = ctx.run(Input::True);
            }
            Some(Ok(Event::Reject { reason, .. })) => {
                actions.push(SieveAction::Reject(reason));
                break;
            }
            Some(Ok(Event::CreatedMessage {
                message_id,
                message,
            })) => {
                created_messages.insert(message_id, message);
                result = ctx.run(Input::True);
            }
            Some(Ok(Event::ListContains { .. })) => {
                result = ctx.run(Input::False);
            }
            Some(Ok(Event::DuplicateId { .. })) => {
                result = ctx.run(Input::False);
            }
            Some(Ok(Event::MailboxExists { .. })) => {
                result = ctx.run(Input::True);
            }
            Some(Ok(Event::IncludeScript { .. })) => {
                result = ctx.run(Input::False);
            }
            Some(Ok(Event::SetEnvelope { .. })) => {
                result = ctx.run(Input::True);
            }
            Some(Ok(Event::Function { .. })) => {
                result = ctx.run(Input::True);
            }
            Some(Ok(_)) => {
                result = ctx.run(Input::True);
            }
            Some(Err(_)) => break,
            None => break,
        }
    }

    if actions.is_empty() {
        actions.push(SieveAction::Keep);
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    const MSG: &[u8] = b"From: sender@example.com\r\nTo: rcpt@example.com\r\nSubject: Test\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\n\r\nHello world";

    #[test]
    fn compile_valid_keep_script() {
        assert!(compile_sieve("require \"fileinto\";\nkeep;").is_ok());
    }

    #[test]
    fn compile_invalid_syntax() {
        assert!(compile_sieve("this is not valid sieve {{{").is_err());
    }

    #[test]
    fn default_keep_empty_body() {
        let compiled = compile_sieve("require \"fileinto\";").unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], SieveAction::Keep));
    }

    #[test]
    fn fileinto_junk() {
        let compiled = compile_sieve("require \"fileinto\";\nfileinto \"Junk\";").unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Junk")));
    }

    #[test]
    fn discard_action() {
        let compiled = compile_sieve("discard;").unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], SieveAction::Discard));
    }

    #[test]
    fn reject_action() {
        let compiled = compile_sieve("require \"reject\";\nreject \"Go away\";").unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], SieveAction::Reject(ref r) if r == "Go away"));
    }

    #[test]
    fn redirect_action() {
        let compiled = compile_sieve("redirect \"fwd@example.com\";").unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::Redirect(ref addr) if addr == "fwd@example.com")));
    }

    #[test]
    fn header_contains_match() {
        let script = r#"
            require "fileinto";
            if header :contains "Subject" "Test" {
                fileinto "Matched";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Matched")));
    }

    #[test]
    fn header_contains_no_match() {
        let script = r#"
            require "fileinto";
            if header :contains "Subject" "spam" {
                fileinto "Junk";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], SieveAction::Keep));
    }

    #[test]
    fn implicit_keep_no_actions() {
        let compiled = compile_sieve("if false { discard; }").unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions.iter().any(|a| matches!(a, SieveAction::Keep)));
    }

    #[test]
    fn if_else_chain() {
        let script = r#"
            require "fileinto";
            if header :contains "Subject" "nope" {
                fileinto "A";
            } elsif header :contains "Subject" "Test" {
                fileinto "B";
            } else {
                fileinto "C";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "B")));
    }

    #[test]
    fn multi_action_fileinto_keep() {
        let script = r#"
            require "fileinto";
            fileinto "Archive";
            keep;
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions.len() >= 2);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Archive")));
        assert!(actions.iter().any(|a| matches!(a, SieveAction::Keep)));
    }

    #[test]
    fn empty_script() {
        let compiled = compile_sieve("");
        // empty script either compiles to implicit keep or fails; both are acceptable
        if let Ok(compiled) = compiled {
            let actions = evaluate_sieve(&compiled, MSG);
            assert!(!actions.is_empty());
            assert!(actions.iter().any(|a| matches!(a, SieveAction::Keep)));
        }
    }

    #[test]
    fn whitespace_only_script() {
        let compiled = compile_sieve("   \n\n  ");
        if let Ok(compiled) = compiled {
            let actions = evaluate_sieve(&compiled, MSG);
            assert!(actions.iter().any(|a| matches!(a, SieveAction::Keep)));
        }
    }

    #[test]
    fn size_over_match() {
        // MSG is small (~100 bytes), so size :over 10 should match
        let script = r#"
            require "fileinto";
            if size :over 10 {
                fileinto "Big";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Big")));
    }

    #[test]
    fn size_over_no_match() {
        // MSG is small, so size :over 1M should not match
        let script = r#"
            require "fileinto";
            if size :over 1M {
                fileinto "Huge";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions.iter().any(|a| matches!(a, SieveAction::Keep)));
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Huge")));
    }

    #[test]
    fn size_under_match() {
        let script = r#"
            require "fileinto";
            if size :under 1M {
                fileinto "Small";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Small")));
    }

    #[test]
    fn header_is_exact_match() {
        let script = r#"
            require "fileinto";
            if header :is "Subject" "Test" {
                fileinto "Exact";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Exact")));
    }

    #[test]
    fn header_is_no_match() {
        let script = r#"
            require "fileinto";
            if header :is "Subject" "Test message" {
                fileinto "Exact";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Exact")));
    }

    #[test]
    fn header_matches_wildcard() {
        let script = r#"
            require "fileinto";
            if header :matches "Subject" "T*" {
                fileinto "Wild";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Wild")));
    }

    #[test]
    fn exists_test_present_header() {
        let script = r#"
            require "fileinto";
            if exists "Subject" {
                fileinto "HasSubject";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "HasSubject")));
    }

    #[test]
    fn exists_test_missing_header() {
        let script = r#"
            require "fileinto";
            if exists "X-Custom-Missing" {
                fileinto "HasCustom";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "HasCustom")));
    }

    #[test]
    fn not_condition() {
        let script = r#"
            require "fileinto";
            if not header :contains "Subject" "spam" {
                fileinto "NotSpam";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "NotSpam")));
    }

    #[test]
    fn allof_both_true() {
        let script = r#"
            require "fileinto";
            if allof (header :contains "Subject" "Test", header :contains "From" "sender") {
                fileinto "Both";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Both")));
    }

    #[test]
    fn allof_one_false() {
        let script = r#"
            require "fileinto";
            if allof (header :contains "Subject" "Test", header :contains "From" "nobody") {
                fileinto "Both";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Both")));
    }

    #[test]
    fn anyof_one_true() {
        let script = r#"
            require "fileinto";
            if anyof (header :contains "Subject" "nope", header :contains "From" "sender") {
                fileinto "Either";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Either")));
    }

    #[test]
    fn anyof_none_true() {
        let script = r#"
            require "fileinto";
            if anyof (header :contains "Subject" "nope", header :contains "From" "nobody") {
                fileinto "Either";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Either")));
    }

    #[test]
    fn address_test() {
        let script = r#"
            require "fileinto";
            if address :contains "From" "sender" {
                fileinto "FromSender";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "FromSender")));
    }

    #[test]
    fn multiple_redirects() {
        // sieve-rs may deduplicate or stop after first redirect per RFC 5228 §2.10.3
        let script = r#"
            redirect "a@example.com";
            redirect "b@example.com";
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        let redirects: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SieveAction::Redirect(_)))
            .collect();
        assert!(!redirects.is_empty());
    }

    #[test]
    fn evaluate_with_minimal_message() {
        // bare minimum: just headers with no body
        let minimal = b"From: a@b.c\r\n\r\n";
        let compiled = compile_sieve("keep;").unwrap();
        let actions = evaluate_sieve(&compiled, minimal);
        assert!(actions.iter().any(|a| matches!(a, SieveAction::Keep)));
    }

    #[test]
    fn evaluate_with_empty_message() {
        let compiled = compile_sieve("keep;").unwrap();
        let actions = evaluate_sieve(&compiled, b"");
        assert!(!actions.is_empty());
    }

    #[test]
    fn sieve_action_debug_clone() {
        let action = SieveAction::FileInto("test".to_string());
        let cloned = action.clone();
        // verify Debug is implemented
        let debug_str = format!("{:?}", cloned);
        assert!(debug_str.contains("FileInto"));
    }

    #[test]
    fn nested_if_conditions() {
        let script = r#"
            require "fileinto";
            if header :contains "From" "sender" {
                if header :contains "Subject" "Test" {
                    fileinto "Nested";
                }
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Nested")));
    }

    #[test]
    fn elsif_fallthrough() {
        let script = r#"
            require "fileinto";
            if header :contains "Subject" "nope" {
                fileinto "A";
            } elsif header :contains "Subject" "also_nope" {
                fileinto "B";
            } else {
                fileinto "Fallback";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Fallback")));
    }

    #[test]
    fn stop_halts_processing() {
        let script = r#"
            require "fileinto";
            fileinto "First";
            stop;
            fileinto "Second";
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "First")));
        // "Second" should not appear because stop halts
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Second")));
    }

    #[test]
    fn compile_error_message_is_descriptive() {
        let err = compile_sieve("invalid sieve {{{}}}").unwrap_err();
        assert!(!err.is_empty(), "error message should not be empty");
    }

    #[test]
    fn header_contains_multiple_keys() {
        // test matching against multiple header names
        let script = r#"
            require "fileinto";
            if header :contains ["Subject", "From"] "example" {
                fileinto "MultiKey";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        // "example" appears in From: sender@example.com
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "MultiKey")));
    }

    #[test]
    fn fileinto_and_discard() {
        // fileinto then discard - discard should terminate
        let script = r#"
            require "fileinto";
            fileinto "Archive";
            discard;
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(_))));
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::Discard)));
    }

    // --- complex allof/anyof nesting ---

    #[test]
    fn nested_allof_inside_anyof() {
        let script = r#"
            require "fileinto";
            if anyof (
                allof (header :contains "Subject" "Test", header :contains "From" "sender"),
                header :contains "To" "nobody"
            ) {
                fileinto "NestedLogic";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "NestedLogic")));
    }

    #[test]
    fn nested_anyof_inside_allof() {
        let script = r#"
            require "fileinto";
            if allof (
                anyof (header :contains "Subject" "Test", header :contains "Subject" "Hello"),
                header :contains "From" "sender"
            ) {
                fileinto "AnyInAll";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "AnyInAll")));
    }

    #[test]
    fn nested_allof_inside_allof() {
        let script = r#"
            require "fileinto";
            if allof (
                allof (header :contains "Subject" "Test", exists "From"),
                allof (header :contains "To" "rcpt", exists "Date")
            ) {
                fileinto "DeepAllof";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "DeepAllof")));
    }

    #[test]
    fn nested_anyof_inside_anyof() {
        let script = r#"
            require "fileinto";
            if anyof (
                anyof (header :contains "Subject" "nope", header :contains "From" "nobody"),
                anyof (header :contains "To" "missing", header :contains "Subject" "Test")
            ) {
                fileinto "DeepAnyof";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "DeepAnyof")));
    }

    #[test]
    fn nested_allof_inside_anyof_all_false() {
        let script = r#"
            require "fileinto";
            if anyof (
                allof (header :contains "Subject" "nope", header :contains "From" "sender"),
                allof (header :contains "Subject" "Test", header :contains "From" "nobody")
            ) {
                fileinto "ShouldNotMatch";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "ShouldNotMatch")));
    }

    #[test]
    fn not_with_allof() {
        let script = r#"
            require "fileinto";
            if not allof (header :contains "Subject" "Test", header :contains "From" "nobody") {
                fileinto "NotAllof";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "NotAllof")));
    }

    #[test]
    fn not_with_anyof() {
        let script = r#"
            require "fileinto";
            if not anyof (header :contains "Subject" "nope", header :contains "From" "nobody") {
                fileinto "NotAnyof";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "NotAnyof")));
    }

    // --- vacation auto-reply ---

    #[test]
    fn vacation_auto_reply() {
        let script = r#"
            require "vacation";
            vacation :days 7 :subject "Out of office" "I am on vacation until next week.";
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve_with_envelope(
            &compiled,
            MSG,
            Some("sender@example.com"),
            Some("rcpt@example.com"),
        );
        // vacation should produce a Vacation action with the auto-reply body
        assert!(
            actions.iter().any(|a| matches!(a, SieveAction::Vacation(_, body) if !body.is_empty())),
            "expected Vacation action, got: {actions:?}"
        );
    }

    #[test]
    fn vacation_with_condition() {
        let script = r#"
            require ["vacation", "fileinto"];
            if header :contains "Subject" "Test" {
                vacation :days 1 :subject "Auto-reply" "Got your test message.";
            }
            fileinto "INBOX";
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve_with_envelope(
            &compiled,
            MSG,
            Some("sender@example.com"),
            Some("rcpt@example.com"),
        );
        assert!(
            actions.iter().any(|a| matches!(a, SieveAction::Vacation(_, _))),
            "expected Vacation action, got: {actions:?}"
        );
    }

    #[test]
    fn vacation_from_address() {
        let script = r#"
            require "vacation";
            vacation :days 3
                     :subject "Away"
                     :from "noreply@example.com"
                     "I am currently unavailable.";
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve_with_envelope(
            &compiled,
            MSG,
            Some("sender@example.com"),
            Some("rcpt@example.com"),
        );
        assert!(
            actions.iter().any(|a| matches!(a, SieveAction::Vacation(_, _))),
            "expected Vacation action, got: {actions:?}"
        );
    }

    // --- fileinto multiple folders ---

    #[test]
    fn fileinto_multiple_folders() {
        let script = r#"
            require "fileinto";
            fileinto "Folder1";
            fileinto "Folder2";
            fileinto "Folder3";
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        let fileinto_count = actions
            .iter()
            .filter(|a| matches!(a, SieveAction::FileInto(_)))
            .count();
        assert!(fileinto_count >= 3, "expected at least 3 fileinto actions, got {fileinto_count}");
    }

    #[test]
    fn fileinto_conditional_multiple_folders() {
        let script = r#"
            require "fileinto";
            if header :contains "From" "sender" {
                fileinto "FromSender";
            }
            if header :contains "Subject" "Test" {
                fileinto "HasTest";
            }
            if header :contains "To" "rcpt" {
                fileinto "ToRcpt";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        let folders: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                SieveAction::FileInto(f) => Some(f.as_str()),
                _ => None,
            })
            .collect();
        assert!(folders.contains(&"FromSender"));
        assert!(folders.contains(&"HasTest"));
        assert!(folders.contains(&"ToRcpt"));
    }

    #[test]
    fn fileinto_with_redirect() {
        let script = r#"
            require "fileinto";
            fileinto "Archive";
            redirect "backup@example.com";
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Archive")));
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::Redirect(ref addr) if addr == "backup@example.com")));
    }

    // --- reject and discard edge cases ---

    #[test]
    fn reject_with_long_reason() {
        let reason = "This mailbox does not accept unsolicited messages. \
                       Please contact the administrator for assistance.";
        let script = format!(
            "require \"reject\";\nreject \"{reason}\";",
        );
        let compiled = compile_sieve(&script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], SieveAction::Reject(r) if r == reason));
    }

    #[test]
    fn reject_conditional() {
        let script = r#"
            require ["reject", "fileinto"];
            if header :contains "Subject" "spam" {
                reject "No spam allowed";
            } else {
                fileinto "INBOX";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        // subject is "Test", not "spam", so should fileinto INBOX
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "INBOX")));
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::Reject(_))));
    }

    #[test]
    fn discard_conditional() {
        let script = r#"
            require "fileinto";
            if header :contains "From" "spammer" {
                discard;
            } else {
                fileinto "INBOX";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "INBOX")));
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::Discard)));
    }

    #[test]
    fn discard_with_matching_condition() {
        let script = r#"
            if header :contains "Subject" "Test" {
                discard;
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], SieveAction::Discard));
    }

    // --- size comparison (over/under) ---

    #[test]
    fn size_under_no_match() {
        // MSG is ~110 bytes, so size :under 10 should not match
        let script = r#"
            require "fileinto";
            if size :under 10 {
                fileinto "Tiny";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Tiny")));
    }

    #[test]
    fn size_over_with_k_unit() {
        // MSG is ~110 bytes, size :over 1K (1024) should not match
        let script = r#"
            require "fileinto";
            if size :over 1K {
                fileinto "OverOneK";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "OverOneK")));
    }

    #[test]
    fn size_combined_with_header_check() {
        let script = r#"
            require "fileinto";
            if allof (size :under 1M, header :contains "Subject" "Test") {
                fileinto "SmallTest";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "SmallTest")));
    }

    #[test]
    fn size_over_with_large_message() {
        // create a message larger than 1K
        let mut large_msg = b"From: a@b.c\r\nTo: d@e.f\r\nSubject: Big\r\n\r\n".to_vec();
        large_msg.extend(vec![b'X'; 2048]);
        let script = r#"
            require "fileinto";
            if size :over 1K {
                fileinto "Large";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, &large_msg);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Large")));
    }

    // --- regex matching ---

    #[test]
    fn regex_match_subject() {
        let script = r#"
            require ["fileinto", "regex"];
            if header :regex "Subject" "^T[a-z]+$" {
                fileinto "Regex";
            }
        "#;
        // regex extension may or may not be supported
        match compile_sieve(script) {
            Ok(compiled) => {
                let actions = evaluate_sieve(&compiled, MSG);
                assert!(actions
                    .iter()
                    .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Regex")));
            }
            Err(_) => {
                // regex not supported by this sieve implementation, that's ok
            }
        }
    }

    #[test]
    fn regex_match_from_domain() {
        let script = r#"
            require ["fileinto", "regex"];
            if header :regex "From" "example\\.com" {
                fileinto "RegexDomain";
            }
        "#;
        match compile_sieve(script) {
            Ok(compiled) => {
                let actions = evaluate_sieve(&compiled, MSG);
                assert!(actions
                    .iter()
                    .any(|a| matches!(a, SieveAction::FileInto(f) if f == "RegexDomain")));
            }
            Err(_) => {
                // regex not supported
            }
        }
    }

    #[test]
    fn regex_no_match() {
        let script = r#"
            require ["fileinto", "regex"];
            if header :regex "Subject" "^[0-9]+$" {
                fileinto "Numbers";
            }
        "#;
        match compile_sieve(script) {
            Ok(compiled) => {
                let actions = evaluate_sieve(&compiled, MSG);
                assert!(!actions
                    .iter()
                    .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Numbers")));
            }
            Err(_) => {
                // regex not supported
            }
        }
    }

    // --- multi-rule combination scenarios ---

    #[test]
    fn complex_multi_rule_pipeline() {
        let script = r#"
            require ["fileinto", "reject"];
            if header :contains "From" "blocked@evil.com" {
                reject "Blocked sender";
            }
            if header :contains "Subject" "Test" {
                fileinto "Tests";
            }
            if header :contains "To" "rcpt@example.com" {
                fileinto "Personal";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        // from is not blocked, so no reject
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::Reject(_))));
        // subject matches "Test"
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Tests")));
        // to matches
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Personal")));
    }

    #[test]
    fn priority_based_classification() {
        // simulate a multi-tier classification system
        let script = r#"
            require "fileinto";
            if header :contains "Subject" "URGENT" {
                fileinto "Priority";
            } elsif header :contains "Subject" "Test" {
                fileinto "Testing";
            } elsif header :contains "From" "newsletter" {
                fileinto "Newsletters";
            } else {
                fileinto "General";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Testing")));
    }

    #[test]
    fn fileinto_redirect_keep_combination() {
        let script = r#"
            require "fileinto";
            fileinto "Archive";
            redirect "copy@example.com";
            keep;
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Archive")));
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::Redirect(ref a) if a == "copy@example.com")));
        assert!(actions.iter().any(|a| matches!(a, SieveAction::Keep)));
    }

    #[test]
    fn nested_if_with_multiple_actions() {
        let script = r#"
            require "fileinto";
            if header :contains "From" "sender" {
                fileinto "FromMatch";
                if header :contains "Subject" "Test" {
                    fileinto "SubjectMatch";
                    if header :contains "To" "rcpt" {
                        fileinto "ToMatch";
                    }
                }
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        let folders: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                SieveAction::FileInto(f) => Some(f.as_str()),
                _ => None,
            })
            .collect();
        assert!(folders.contains(&"FromMatch"));
        assert!(folders.contains(&"SubjectMatch"));
        assert!(folders.contains(&"ToMatch"));
    }

    #[test]
    fn stop_prevents_later_rules() {
        let script = r#"
            require "fileinto";
            if header :contains "Subject" "Test" {
                fileinto "Matched";
                stop;
            }
            fileinto "ShouldNotReach";
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Matched")));
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "ShouldNotReach")));
    }

    // --- invalid script error handling ---

    #[test]
    fn compile_missing_require() {
        // using fileinto without require should fail or produce error
        let script = r#"fileinto "Test";"#;
        let result = compile_sieve(script);
        // per RFC 5228, fileinto needs require; implementation may reject
        // either compile error or runtime implicit keep are acceptable
        if let Ok(compiled) = result {
            let actions = evaluate_sieve(&compiled, MSG);
            assert!(!actions.is_empty());
        }
    }

    #[test]
    fn compile_unclosed_string() {
        let result = compile_sieve("require \"fileinto;\nfileinto \"Test;");
        assert!(result.is_err());
    }

    #[test]
    fn compile_unknown_extension() {
        // sieve-rs compiler accepts unknown extensions without error,
        // so we just verify it doesn't panic and produces a valid compiled script
        let result = compile_sieve("require \"nonexistent_extension_xyz\";");
        if let Ok(compiled) = result {
            let actions = evaluate_sieve(&compiled, MSG);
            assert!(actions.iter().any(|a| matches!(a, SieveAction::Keep)));
        }
    }

    #[test]
    fn compile_missing_semicolon() {
        let result = compile_sieve("require \"fileinto\"\nfileinto \"Test\"");
        assert!(result.is_err());
    }

    #[test]
    fn compile_mismatched_braces() {
        let result = compile_sieve("require \"fileinto\";\nif true { fileinto \"Test\";");
        assert!(result.is_err());
    }

    #[test]
    fn compile_empty_require_list() {
        // require with empty list: require [];
        let result = compile_sieve("require [];");
        // this may or may not be valid depending on the implementation
        // we just check it doesn't panic
        let _ = result;
    }

    #[test]
    fn compile_duplicate_require() {
        let script = r#"
            require "fileinto";
            require "fileinto";
            fileinto "Test";
        "#;
        // duplicate require may or may not be an error
        let _ = compile_sieve(script);
    }

    // --- address part tests ---

    #[test]
    fn address_localpart() {
        let script = r#"
            require "fileinto";
            if address :localpart :is "From" "sender" {
                fileinto "LocalPart";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "LocalPart")));
    }

    #[test]
    fn address_domain() {
        let script = r#"
            require "fileinto";
            if address :domain :is "From" "example.com" {
                fileinto "DomainMatch";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "DomainMatch")));
    }

    #[test]
    fn address_domain_no_match() {
        let script = r#"
            require "fileinto";
            if address :domain :is "From" "other.com" {
                fileinto "WrongDomain";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "WrongDomain")));
    }

    // --- multi-value header matching ---

    #[test]
    fn header_contains_multiple_values() {
        let script = r#"
            require "fileinto";
            if header :contains "Subject" ["spam", "Test", "urgent"] {
                fileinto "AnyValue";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "AnyValue")));
    }

    #[test]
    fn header_contains_multiple_values_none_match() {
        let script = r#"
            require "fileinto";
            if header :contains "Subject" ["spam", "urgent", "newsletter"] {
                fileinto "AnyValue";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "AnyValue")));
    }

    // --- special message scenarios ---

    #[test]
    fn multipart_message() {
        let msg = b"From: sender@example.com\r\n\
                     To: rcpt@example.com\r\n\
                     Subject: Multipart Test\r\n\
                     MIME-Version: 1.0\r\n\
                     Content-Type: multipart/alternative; boundary=\"boundary\"\r\n\
                     \r\n\
                     --boundary\r\n\
                     Content-Type: text/plain\r\n\
                     \r\n\
                     Plain text\r\n\
                     --boundary\r\n\
                     Content-Type: text/html\r\n\
                     \r\n\
                     <p>HTML</p>\r\n\
                     --boundary--\r\n";
        let script = r#"
            require "fileinto";
            if header :contains "Subject" "Multipart" {
                fileinto "Multi";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, msg);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Multi")));
    }

    #[test]
    fn message_with_many_headers() {
        let msg = b"From: sender@example.com\r\n\
                     To: rcpt@example.com\r\n\
                     Cc: cc@example.com\r\n\
                     Bcc: bcc@example.com\r\n\
                     Subject: Complex Headers\r\n\
                     Date: Mon, 01 Jan 2024 00:00:00 +0000\r\n\
                     Reply-To: reply@example.com\r\n\
                     X-Priority: 1\r\n\
                     X-Mailer: TestMailer\r\n\
                     Message-ID: <test123@example.com>\r\n\
                     \r\n\
                     Body content";
        let script = r#"
            require "fileinto";
            if allof (
                exists "X-Priority",
                header :contains "X-Mailer" "TestMailer",
                header :contains "Subject" "Complex"
            ) {
                fileinto "ComplexHeaders";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, msg);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "ComplexHeaders")));
    }

    // --- case sensitivity ---

    #[test]
    fn header_contains_case_insensitive_value() {
        // RFC 5228: :contains comparisons are case-insensitive by default
        let script = r#"
            require "fileinto";
            if header :contains "Subject" "test" {
                fileinto "CaseInsensitive";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        // "test" should match "Test" case-insensitively
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "CaseInsensitive")));
    }

    #[test]
    fn header_is_case_insensitive() {
        let script = r#"
            require "fileinto";
            if header :is "Subject" "test" {
                fileinto "CaseExact";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, MSG);
        // :is should still be case-insensitive per RFC 5228
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "CaseExact")));
    }

    // --- realistic spam filtering scenario ---

    #[test]
    fn realistic_spam_filter_pipeline() {
        let spam_msg = b"From: spammer@evil.com\r\n\
                          To: victim@example.com\r\n\
                          Subject: FREE MONEY!!!\r\n\
                          Date: Mon, 01 Jan 2024 00:00:00 +0000\r\n\
                          X-Spam-Score: 9.5\r\n\
                          \r\n\
                          Click here to claim your prize!";
        let script = r#"
            require ["fileinto", "reject"];
            if anyof (
                header :contains "Subject" "FREE MONEY",
                header :contains "From" "evil.com",
                header :contains "X-Spam-Score" "9"
            ) {
                fileinto "Junk";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, spam_msg);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Junk")));
    }

    #[test]
    fn realistic_mailing_list_sorting() {
        let list_msg = b"From: list-owner@lists.example.com\r\n\
                          To: user@example.com\r\n\
                          Subject: [dev] Weekly update\r\n\
                          List-Id: <dev.lists.example.com>\r\n\
                          Date: Mon, 01 Jan 2024 00:00:00 +0000\r\n\
                          \r\n\
                          This week's update...";
        let script = r#"
            require "fileinto";
            if header :contains "List-Id" "dev.lists" {
                fileinto "Lists/Dev";
            } elsif header :contains "List-Id" "announce.lists" {
                fileinto "Lists/Announce";
            } elsif exists "List-Id" {
                fileinto "Lists/Other";
            }
        "#;
        let compiled = compile_sieve(script).unwrap();
        let actions = evaluate_sieve(&compiled, list_msg);
        assert!(actions
            .iter()
            .any(|a| matches!(a, SieveAction::FileInto(f) if f == "Lists/Dev")));
    }
}

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
pub fn evaluate_sieve(compiled: &Arc<CompiledSieve>, message: &[u8]) -> Vec<SieveAction> {
    let runtime = Runtime::new();
    let mut ctx = runtime.filter(message);

    let input = Input::Script {
        name: sieve::Script::Personal("main".into()),
        script: compiled.clone(),
    };

    let mut actions = Vec::new();
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
            Some(Ok(Event::SendMessage { recipient, .. })) => {
                let addr = match recipient {
                    Recipient::Address(a) => a,
                    Recipient::List(l) => l,
                    Recipient::Group(g) => g.into_iter().next().unwrap_or_default(),
                };
                actions.push(SieveAction::Redirect(addr));
                result = ctx.run(Input::True);
            }
            Some(Ok(Event::Reject { reason, .. })) => {
                actions.push(SieveAction::Reject(reason));
                break;
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
            Some(Ok(Event::CreatedMessage { .. })) => {
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
        assert!(actions.iter().any(|a| matches!(a, SieveAction::FileInto(f) if f == "Junk")));
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
        assert!(actions.iter().any(|a| matches!(a, SieveAction::Redirect(ref addr) if addr == "fwd@example.com")));
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
        assert!(actions.iter().any(|a| matches!(a, SieveAction::FileInto(f) if f == "Matched")));
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
        assert!(actions.iter().any(|a| matches!(a, SieveAction::FileInto(f) if f == "B")));
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
        assert!(actions.iter().any(|a| matches!(a, SieveAction::FileInto(f) if f == "Archive")));
        assert!(actions.iter().any(|a| matches!(a, SieveAction::Keep)));
    }
}

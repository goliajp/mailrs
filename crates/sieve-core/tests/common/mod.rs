//! Shared differential-test helpers — `NormalizedAction` + the
//! two engine wrappers (`ours` calling `sieve-core`, `sieve_rs`
//! calling the oracle crate). Extracted into a `common` module so
//! `tests/diff_sieve_rs.rs` stays under the file-size limit.

#![allow(dead_code)]

use std::sync::Arc;

use mailrs_sieve_core::{Action, eval_script};
use sieve::{Compiler, Event, Input, Recipient, Runtime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedAction {
    Keep,
    Discard,
    FileInto(String),
    Redirect(String),
    Reject(String),
}

pub fn ours(script: &str, msg: &[u8]) -> Vec<NormalizedAction> {
    let actions = eval_script(script, msg).unwrap_or_default();
    actions
        .into_iter()
        .map(|a| match a {
            Action::Keep => NormalizedAction::Keep,
            Action::Discard => NormalizedAction::Discard,
            Action::FileInto(s) => NormalizedAction::FileInto(s),
            Action::Redirect(s) => NormalizedAction::Redirect(s),
            Action::Reject(s) => NormalizedAction::Reject(s),
            // Vacation is intentionally excluded from the differential
            // corpus — sieve-rs internalises message-building while
            // sieve-core surfaces an abstract action, so the
            // abstractions don't line up. RFC 5230 spec coverage is
            // in vacation.rs's inline unit tests instead.
            Action::Vacation(_) => {
                panic!("vacation is excluded from the differential corpus; see vacation.rs tests")
            }
        })
        .collect()
}

pub fn sieve_rs(script: &str, msg: &[u8]) -> Vec<NormalizedAction> {
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

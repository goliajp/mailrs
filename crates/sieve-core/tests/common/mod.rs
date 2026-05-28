//! Shared differential-test helpers — `NormalizedAction` + the
//! two engine wrappers (`ours` calling `sieve-core`, `sieve_rs`
//! calling the oracle crate) + the differential corpus. Extracted
//! into a `common` module so `tests/diff_sieve_rs.rs` stays under
//! the file-size limit.

#![allow(dead_code)]

use std::sync::Arc;

use mailrs_sieve_core::{Action, eval_script};
use sieve::{Compiler, Event, Input, Recipient, Runtime};

pub mod corpus;

/// One corpus row: label + Sieve script + sample message bytes.
pub type CorpusRow = (&'static str, &'static str, &'static [u8]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedAction {
    Keep { flags: Vec<String> },
    Discard,
    FileInto { folder: String, flags: Vec<String> },
    Redirect(String),
    Reject(String),
}

pub fn ours(script: &str, msg: &[u8]) -> Vec<NormalizedAction> {
    let actions = eval_script(script, msg).unwrap_or_default();
    actions
        .into_iter()
        .map(|a| match a {
            Action::Keep { flags } => NormalizedAction::Keep { flags: sort_flags(flags) },
            Action::Discard => NormalizedAction::Discard,
            Action::FileInto { mailbox, flags } => NormalizedAction::FileInto {
                folder: mailbox,
                flags: sort_flags(flags),
            },
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

/// IMAP flags have no defined order in RFC 5232 — the two engines
/// may emit them in different orders depending on internal data
/// structures. Sort for fair comparison.
fn sort_flags(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

pub fn sieve_rs(script: &str, msg: &[u8]) -> Vec<NormalizedAction> {
    let compiler = Compiler::new();
    let compiled = match compiler.compile(script.as_bytes()) {
        Ok(c) => Arc::new(c),
        Err(_) => return Vec::new(),
    };
    // sieve-rs defaults `max_redirects = 1` as an anti-mail-loop
    // policy. That's a caller-policy choice, not an RFC 5228
    // requirement — sieve-core (zero-I/O stone) leaves the
    // decision to the caller. Lift the cap here so the
    // differential test compares spec behaviour, not policy.
    let runtime = Runtime::new().with_max_redirects(usize::MAX);
    let mut ctx = runtime.filter(msg);
    let input = Input::Script {
        name: sieve::Script::Personal("main".into()),
        script: compiled,
    };
    let mut out = Vec::new();
    let mut result = ctx.run(input);
    loop {
        match result {
            Some(Ok(Event::Keep { flags, .. })) => {
                out.push(NormalizedAction::Keep { flags: sort_flags(flags) });
                break;
            }
            Some(Ok(Event::Discard)) => {
                out.push(NormalizedAction::Discard);
                break;
            }
            Some(Ok(Event::FileInto { folder, flags, .. })) => {
                out.push(NormalizedAction::FileInto { folder, flags: sort_flags(flags) });
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

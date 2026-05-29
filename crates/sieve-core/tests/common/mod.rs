//! Shared golden-test helpers — `NormalizedAction` + the `ours`
//! engine wrapper (calling `sieve-core`) + the corpus. The AGPL
//! `sieve-rs` oracle was dropped in v8 ckpt 6; expected output is now
//! frozen in `golden.txt`.

#![allow(dead_code)]

use mailrs_sieve_core::{Action, Envelope, eval_script, eval_script_with_envelope};

pub mod corpus;

/// One corpus row: label + Sieve script + sample message bytes.
pub type CorpusRow = (&'static str, &'static str, &'static [u8]);

/// One envelope-aware corpus row: label + script + message +
/// envelope entries (part name + value).
pub type EnvelopeRow = (
    &'static str,
    &'static str,
    &'static [u8],
    &'static [(&'static str, &'static str)],
);

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
    normalize(actions)
}

/// IMAP flags have no defined order in RFC 5232 — sort for stable
/// golden output.
fn sort_flags(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

/// `ours` with explicit envelope context for the RFC 5228 §5.4
/// envelope test. `entries` is a slice of `(part_name, value)` where
/// part_name is one of "from" / "to" / "auth" (case-insensitive).
pub fn ours_with_envelope(
    script: &str,
    msg: &[u8],
    entries: &[(&str, &str)],
) -> Vec<NormalizedAction> {
    let env = build_envelope(entries);
    let actions = eval_script_with_envelope(script, msg, &env).unwrap_or_default();
    normalize(actions)
}

fn build_envelope(entries: &[(&str, &str)]) -> Envelope {
    let mut env = Envelope::default();
    for (part, value) in entries {
        match part.to_ascii_lowercase().as_str() {
            "from" => env.from = Some((*value).into()),
            "to" => env.to.push((*value).into()),
            "auth" => env.auth = Some((*value).into()),
            _ => {}
        }
    }
    env
}

fn normalize(actions: Vec<Action>) -> Vec<NormalizedAction> {
    actions
        .into_iter()
        .map(|a| match a {
            Action::Keep { flags } => NormalizedAction::Keep {
                flags: sort_flags(flags),
            },
            Action::Discard => NormalizedAction::Discard,
            Action::FileInto { mailbox, flags } => NormalizedAction::FileInto {
                folder: mailbox,
                flags: sort_flags(flags),
            },
            Action::Redirect(s) => NormalizedAction::Redirect(s),
            Action::Reject(s) => NormalizedAction::Reject(s),
            Action::Vacation(_) => {
                panic!("vacation is excluded from the differential corpus; see vacation.rs tests")
            }
        })
        .collect()
}

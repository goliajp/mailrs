//! RFC 5228 §5 test-evaluation engine — extracted from `eval/mod.rs`
//! so the command-dispatch loop stays under the file-size limit.
//!
//! Public entry: `eval_test(test, ctx)`. The two arg-shaping
//! helpers (`pair_lists`, `arg_strings_or_list`) live here because
//! they're only used by tests, not by command dispatch.

use crate::address::{address_part_from_tags, extract_addresses, scope_to_part};
use crate::ast::{Argument, Envelope, MatchType, Test};
use crate::match_str::match_string;

use super::EvalError;
use super::context::MessageContext;

pub(super) fn eval_test(
    t: &Test,
    ctx: &MessageContext<'_>,
    flags: &[String],
    envelope: &Envelope,
) -> Result<bool, EvalError> {
    match t.name.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        "not" => Ok(!eval_test(&t.children[0], ctx, flags, envelope)?),
        "allof" => {
            for c in &t.children {
                if !eval_test(c, ctx, flags, envelope)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        "anyof" => {
            for c in &t.children {
                if eval_test(c, ctx, flags, envelope)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        "exists" => {
            let names = arg_strings_or_list(&t.args);
            Ok(names.iter().all(|n| !ctx.header_values(n).is_empty()))
        }
        "size" => eval_size(t, ctx),
        "header" => Ok(eval_header(t, ctx)),
        "address" => Ok(eval_address(t, ctx)),
        "hasflag" => Ok(eval_hasflag(t, flags)),
        "envelope" => Ok(eval_envelope_test(t, envelope)),
        other => Err(EvalError::UnknownTest(other.to_string())),
    }
}

/// RFC 5228 §5.4 `envelope` test. Inspects the caller-supplied
/// `from` / `to` / `auth` envelope state with the requested
/// match-type + address-part semantics.
fn eval_envelope_test(t: &Test, envelope: &Envelope) -> bool {
    let mt = MatchType::from_tags(&t.tags);
    let part = address_part_from_tags(&t.tags);
    let (parts, values) = pair_lists(&t.args);
    for part_name in &parts {
        let candidates = envelope_field(envelope, part_name);
        for addr in candidates {
            let Some(scoped) = scope_to_part(&addr, part) else {
                continue; // undefined :detail (no `+`) → skip
            };
            for needle in &values {
                if match_string(mt, &scoped, needle) {
                    return true;
                }
            }
        }
    }
    false
}

/// Resolve `from` / `to` / `auth` (RFC 5228 §5.4, case-insensitive
/// part name) into the candidate address strings for matching.
fn envelope_field(envelope: &Envelope, name: &str) -> Vec<String> {
    match name.to_ascii_lowercase().as_str() {
        "from" => envelope.from.clone().into_iter().collect(),
        "to" => envelope.to.clone(),
        "auth" => envelope.auth.clone().into_iter().collect(),
        _ => Vec::new(),
    }
}

/// RFC 5232 §5.4 `hasflag` — true iff any flag in the test arg
/// list matches any flag in the implicit flags variable, using
/// the requested match-type (defaults to `:is`, case-insensitive).
fn eval_hasflag(t: &Test, flags: &[String]) -> bool {
    let mt = MatchType::from_tags(&t.tags);
    let needles = match t.args.first() {
        Some(Argument::String(s)) => vec![s.clone()],
        Some(Argument::StringList(v)) => v.clone(),
        _ => return false,
    };
    for needle in &needles {
        for held in flags {
            if match_string(mt, held, needle) {
                return true;
            }
        }
    }
    false
}

fn eval_size(t: &Test, ctx: &MessageContext<'_>) -> Result<bool, EvalError> {
    let n = match t.args.first() {
        Some(Argument::Number(n)) => *n,
        _ => {
            return Err(EvalError::BadArg {
                cmd: "size".into(),
                detail: "expects numeric size".into(),
            });
        }
    };
    let size = ctx.body_size();
    let under = t.tags.iter().any(|s| s == "under");
    Ok(if under { size < n } else { size > n })
}

fn eval_header(t: &Test, ctx: &MessageContext<'_>) -> bool {
    let mt = MatchType::from_tags(&t.tags);
    let (names, values) = pair_lists(&t.args);
    for name in &names {
        for hv in ctx.header_values(name) {
            for needle in &values {
                if match_string(mt, &hv, needle) {
                    return true;
                }
            }
        }
    }
    false
}

fn eval_address(t: &Test, ctx: &MessageContext<'_>) -> bool {
    let mt = MatchType::from_tags(&t.tags);
    let part = address_part_from_tags(&t.tags);
    let (names, values) = pair_lists(&t.args);
    for name in &names {
        for hv in ctx.header_values(name) {
            for addr in extract_addresses(&hv) {
                let Some(scoped) = scope_to_part(&addr, part) else {
                    continue; // undefined :detail (no `+`) → skip
                };
                for needle in &values {
                    if match_string(mt, &scoped, needle) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Collect every `String` / `StringList` arg into a flat vector
/// (skip tags, numbers, nested tests). Used by `exists`.
fn arg_strings_or_list(args: &[Argument]) -> Vec<String> {
    let mut out = Vec::new();
    for a in args {
        match a {
            Argument::String(s) => out.push(s.clone()),
            Argument::StringList(v) => out.extend(v.iter().cloned()),
            _ => {}
        }
    }
    out
}

/// Split a header/address test's args into `(names, values)`: the
/// first non-tag arg (string-or-list) is the header names, the
/// second is the values to match against.
fn pair_lists(args: &[Argument]) -> (Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut values = Vec::new();
    let mut idx = 0usize;
    for a in args {
        let collected = match a {
            Argument::String(s) => vec![s.clone()],
            Argument::StringList(v) => v.clone(),
            _ => continue,
        };
        if idx == 0 {
            names = collected;
        } else if idx == 1 {
            values = collected;
        }
        idx += 1;
    }
    (names, values)
}

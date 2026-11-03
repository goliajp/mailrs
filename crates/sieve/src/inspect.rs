//! What a script could do, read from its text without a message: every
//! address it can redirect to and every `:from` its vacation replies can
//! carry, on every branch. A store checks these before it accepts a
//! script, since a branch that fires only on some future message is as
//! much the owner's choice as one that fires on every message.

use mailrs_sieve_core::{Argument, Command, parse_script, parse_vacation_args};

/// Addresses and senders a script names, in source order, each once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reach {
    /// Every `redirect` target.
    pub redirects: Vec<String>,
    /// Every vacation `:from`.
    pub vacation_froms: Vec<String>,
}

/// What `script` could do. An error when it does not parse.
pub fn reach(script: &str) -> Result<Reach, String> {
    let commands = parse_script(script).map_err(|e| format!("{e}"))?;
    let mut out = Reach::default();
    walk(&commands, &mut out);
    Ok(out)
}

fn walk(commands: &[Command], out: &mut Reach) {
    for cmd in commands {
        match cmd.name.to_ascii_lowercase().as_str() {
            "redirect" => {
                if let Some(target) = first_string(&cmd.args) {
                    push_once(&mut out.redirects, target.trim());
                }
            }
            "vacation" => {
                if let Some(from) = parse_vacation_args(&cmd.args).ok().and_then(|v| v.from) {
                    push_once(&mut out.vacation_froms, from.trim());
                }
            }
            _ => {}
        }
        walk(&cmd.block, out);
    }
}

/// The first positional string, a tag's value skipped, as the evaluator
/// reads a redirect's target.
fn first_string(args: &[Argument]) -> Option<&str> {
    let mut i = 0;
    while i < args.len() {
        match &args[i] {
            Argument::Tag(_) => i += 2,
            Argument::String(s) => return Some(s.as_str()),
            _ => i += 1,
        }
    }
    None
}

fn push_once(list: &mut Vec<String>, value: &str) {
    if !value.is_empty() && !list.iter().any(|v| v.eq_ignore_ascii_case(value)) {
        list.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_on_every_branch_are_found() {
        let r = reach(
            r#"if header :contains "subject" "x" { redirect "a@out.test"; }
               elsif true { if false { redirect "b@out.test"; } }
               else { redirect "A@out.test"; }"#,
        )
        .unwrap();
        assert_eq!(r.redirects, ["a@out.test", "b@out.test"]);
        assert!(r.vacation_froms.is_empty());
    }

    #[test]
    fn a_vacation_from_is_found_and_one_without_is_not() {
        let r = reach(
            r#"require "vacation";
               vacation :from "ceo@x.test" :days 1 "away";
               if true { vacation "also away"; }"#,
        )
        .unwrap();
        assert_eq!(r.vacation_froms, ["ceo@x.test"]);
        assert!(r.redirects.is_empty());
    }

    #[test]
    fn a_script_that_does_not_parse_is_an_error() {
        assert!(reach("redirect {").is_err());
    }
}

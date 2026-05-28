//! RFC 5228 §4 evaluator — walk the AST against a parsed message,
//! emit a list of `Action`s.

mod context;
mod test_engine;

use crate::ast::{Action, Argument, Command, Test};
use crate::parse::{ParseError, parse_script};
use crate::vacation::parse_vacation_args;

use context::MessageContext;
use test_engine::eval_test;

/// Eval failure modes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// Parse failed before evaluation could start.
    #[error("parse: {0}")]
    Parse(#[from] ParseError),
    /// Unknown command in the script body (RFC 5228 §6.7
    /// requires `require` to whitelist any non-base command).
    #[error("unknown command {0:?}")]
    UnknownCommand(String),
    /// Unknown test inside a control-flow expression.
    #[error("unknown test {0:?}")]
    UnknownTest(String),
    /// A command argument was the wrong shape for the command.
    #[error("invalid argument for {cmd:?}: {detail}")]
    BadArg {
        /// Name of the command whose arg validation failed.
        cmd: String,
        /// Human-readable explanation.
        detail: String,
    },
}

/// Evaluate a Sieve script against a message. Returns the
/// action list the delivery layer should apply, or `[Keep]` if
/// no script action fired (the RFC 5228 §2.10.6 implicit keep).
pub fn eval_script(script: &str, message: &[u8]) -> Result<Vec<Action>, EvalError> {
    let commands = parse_script(script)?;
    let ctx = MessageContext::new(message);
    let mut state = EvalState::default();
    eval_block(&commands, &ctx, &mut state)?;
    if !state.explicit_action {
        state.actions.push(Action::Keep {
            flags: state.flags.clone(),
        });
    }
    Ok(state.actions)
}

#[derive(Default)]
struct EvalState {
    actions: Vec<Action>,
    explicit_action: bool,
    /// Index of the most recent `if`/`elsif` chain's outcome.
    /// Once an `if` branch matches, subsequent `elsif`/`else` are
    /// skipped per RFC 5228 §3.1.
    last_if_matched: bool,
    /// RFC 5228 §3.3 `stop` — terminate evaluation, do not run any
    /// subsequent commands in any enclosing block.
    stopped: bool,
    /// RFC 5232 `imap4flags` implicit flags variable. Mutated by
    /// `setflag` / `addflag` / `removeflag`; snapshotted into the
    /// next `keep` / `fileinto` action.
    flags: Vec<String>,
}

fn eval_block(
    commands: &[Command],
    ctx: &MessageContext<'_>,
    state: &mut EvalState,
) -> Result<(), EvalError> {
    for cmd in commands {
        if state.stopped {
            break;
        }
        eval_command(cmd, ctx, state)?;
    }
    Ok(())
}

fn eval_command(
    cmd: &Command,
    ctx: &MessageContext<'_>,
    state: &mut EvalState,
) -> Result<(), EvalError> {
    match cmd.name.as_str() {
        "require" => Ok(()), // capabilities are advisory in the v0.1 evaluator
        "keep" => {
            let local = override_flags_from_tag(&cmd.args);
            state.actions.push(Action::Keep {
                flags: local.unwrap_or_else(|| state.flags.clone()),
            });
            state.explicit_action = true;
            Ok(())
        }
        "discard" => {
            state.actions.push(Action::Discard);
            state.explicit_action = true;
            Ok(())
        }
        "fileinto" => {
            let arg = first_string(&cmd.args).ok_or_else(|| EvalError::BadArg {
                cmd: "fileinto".into(),
                detail: "expects string mailbox name".into(),
            })?;
            let local = override_flags_from_tag(&cmd.args);
            state.actions.push(Action::FileInto {
                mailbox: arg.to_string(),
                flags: local.unwrap_or_else(|| state.flags.clone()),
            });
            state.explicit_action = true;
            Ok(())
        }
        "setflag" => {
            // RFC 5232 §4.4: replace the implicit flags variable.
            // Does NOT count as explicit_action (state mutation,
            // not delivery).
            state.flags = flag_list_from_args(&cmd.args);
            Ok(())
        }
        "addflag" => {
            // RFC 5232 §4.5: add flags (no duplicates).
            for f in flag_list_from_args(&cmd.args) {
                if !state.flags.iter().any(|existing| existing == &f) {
                    state.flags.push(f);
                }
            }
            Ok(())
        }
        "removeflag" => {
            // RFC 5232 §4.6: remove matching flags.
            let to_remove = flag_list_from_args(&cmd.args);
            state.flags.retain(|f| !to_remove.iter().any(|r| r == f));
            Ok(())
        }
        "redirect" => {
            let arg = first_string(&cmd.args).ok_or_else(|| EvalError::BadArg {
                cmd: "redirect".into(),
                detail: "expects string address".into(),
            })?;
            state.actions.push(Action::Redirect(arg.to_string()));
            state.explicit_action = true;
            Ok(())
        }
        "reject" => {
            let arg = first_string(&cmd.args).ok_or_else(|| EvalError::BadArg {
                cmd: "reject".into(),
                detail: "expects string reason".into(),
            })?;
            state.actions.push(Action::Reject(arg.to_string()));
            state.explicit_action = true;
            Ok(())
        }
        "stop" => {
            // RFC 5228 §4.5 — terminate evaluation but do NOT
            // cancel the implicit keep (leave `explicit_action`
            // unchanged).
            state.stopped = true;
            Ok(())
        }
        "vacation" => {
            // RFC 5230 §3: vacation emits an action but does NOT
            // cancel the implicit keep — leave `explicit_action`
            // unchanged.
            let va = parse_vacation_args(&cmd.args).map_err(|e| EvalError::BadArg {
                cmd: "vacation".into(),
                detail: e.to_string(),
            })?;
            state.actions.push(Action::Vacation(va));
            Ok(())
        }
        "if" => {
            let test = first_test(&cmd.args).ok_or_else(|| EvalError::BadArg {
                cmd: "if".into(),
                detail: "expects test expression".into(),
            })?;
            let matched = eval_test(test, ctx, &state.flags)?;
            state.last_if_matched = matched;
            if matched {
                eval_block(&cmd.block, ctx, state)?;
            }
            Ok(())
        }
        "elsif" => {
            if state.last_if_matched {
                // chain already matched — skip
                return Ok(());
            }
            let test = first_test(&cmd.args).ok_or_else(|| EvalError::BadArg {
                cmd: "elsif".into(),
                detail: "expects test expression".into(),
            })?;
            let matched = eval_test(test, ctx, &state.flags)?;
            state.last_if_matched = matched;
            if matched {
                eval_block(&cmd.block, ctx, state)?;
            }
            Ok(())
        }
        "else" => {
            if state.last_if_matched {
                return Ok(());
            }
            state.last_if_matched = true;
            eval_block(&cmd.block, ctx, state)?;
            Ok(())
        }
        other => Err(EvalError::UnknownCommand(other.to_string())),
    }
}

/// Find the first **positional** string argument, skipping any
/// `:tag <value>` pairs (e.g. `:flags "\Seen"` in RFC 5232
/// `fileinto :flags "\Seen" "Inbox"`).
fn first_string(args: &[Argument]) -> Option<&str> {
    let mut i = 0;
    while i < args.len() {
        match &args[i] {
            Argument::Tag(_) => {
                // A tag consumes its value argument; jump past both.
                i += 2;
            }
            Argument::String(s) => return Some(s.as_str()),
            _ => i += 1,
        }
    }
    None
}

fn first_test(args: &[Argument]) -> Option<&Test> {
    args.iter().find_map(|a| match a {
        Argument::Test(t) => Some(t),
        _ => None,
    })
}

/// RFC 5232 `:flags` tag handling for `keep` / `fileinto`. Returns
/// `Some(list)` if the command carries an explicit `:flags`
/// argument (which overrides the implicit flags variable); returns
/// `None` if no tag is present (caller falls back to the variable).
fn override_flags_from_tag(args: &[Argument]) -> Option<Vec<String>> {
    for (i, a) in args.iter().enumerate() {
        if matches!(a, Argument::Tag(t) if t == "flags") {
            return match args.get(i + 1) {
                Some(Argument::String(s)) => Some(vec![s.clone()]),
                Some(Argument::StringList(v)) => Some(v.clone()),
                _ => Some(Vec::new()),
            };
        }
    }
    None
}

/// Extract the flag list from `setflag` / `addflag` / `removeflag`
/// command args (RFC 5232 §4.4–4.6). Accepts either a single string
/// or a string-list; variable-name form is not yet supported.
fn flag_list_from_args(args: &[Argument]) -> Vec<String> {
    for a in args {
        match a {
            Argument::String(s) => return vec![s.clone()],
            Argument::StringList(v) => return v.clone(),
            _ => {}
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MSG: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com, carol@dest.com\r\n\
Subject: spam offer\r\n\
\r\n\
hello world\r\n";

    #[test]
    fn implicit_keep_on_empty_script() {
        assert_eq!(eval_script("", MSG).unwrap(), vec![Action::Keep { flags: vec![] }]);
    }

    #[test]
    fn explicit_keep() {
        assert_eq!(eval_script("keep;", MSG).unwrap(), vec![Action::Keep { flags: vec![] }]);
    }

    #[test]
    fn discard() {
        assert_eq!(eval_script("discard;", MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn fileinto() {
        let script = r#"require ["fileinto"]; fileinto "Junk";"#;
        assert_eq!(
            eval_script(script, MSG).unwrap(),
            vec![Action::FileInto { mailbox: "Junk".into(), flags: vec![] }]
        );
    }

    #[test]
    fn header_is_match_fires_discard() {
        let script = r#"if header :is "Subject" "spam offer" { discard; }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn header_is_no_match_falls_through_to_keep() {
        let script = r#"if header :is "Subject" "newsletter" { discard; }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Keep { flags: vec![] }]);
    }

    #[test]
    fn header_contains_substring() {
        let script = r#"if header :contains "Subject" "offer" { fileinto "Ads"; }"#;
        assert_eq!(
            eval_script(script, MSG).unwrap(),
            vec![Action::FileInto { mailbox: "Ads".into(), flags: vec![] }]
        );
    }

    #[test]
    fn header_matches_glob() {
        let script = r#"if header :matches "Subject" "*offer*" { discard; }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn exists_test_present() {
        let script = r#"if exists "Subject" { discard; }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn exists_test_missing() {
        let script = r#"if exists "X-Spam-Score" { discard; }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Keep { flags: vec![] }]);
    }

    #[test]
    fn size_over() {
        let script = "if size :over 1 { discard; }";
        // body is way bigger than 1 byte
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn size_under_huge() {
        let script = "if size :under 100K { discard; }";
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn not_invert() {
        let script = r#"if not header :is "Subject" "newsletter" { discard; }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn allof_true() {
        let script = r#"
            if allof(
                header :contains "Subject" "spam",
                header :is "From" "Alice <alice@example.com>"
            ) {
                discard;
            }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn anyof_one_true() {
        let script = r#"
            if anyof(
                header :is "Subject" "newsletter",
                header :contains "From" "alice"
            ) {
                discard;
            }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn elsif_chain_only_first_matching_branch_fires() {
        let script = r#"
            require ["fileinto"];
            if header :is "Subject" "no-match" { fileinto "A"; }
            elsif header :contains "Subject" "spam" { fileinto "Spam"; }
            elsif header :contains "Subject" "offer" { fileinto "Ads"; }
            else { keep; }"#;
        assert_eq!(
            eval_script(script, MSG).unwrap(),
            vec![Action::FileInto { mailbox: "Spam".into(), flags: vec![] }]
        );
    }

    #[test]
    fn else_branch() {
        let script = r#"
            if header :is "Subject" "no-match" { discard; }
            else { keep; }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Keep { flags: vec![] }]);
    }

    #[test]
    fn address_localpart() {
        let script = r#"if address :localpart "From" "alice" { discard; }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Discard]);
    }

    #[test]
    fn address_domain() {
        let script = r#"if address :domain "To" "dest.com" { fileinto "Sent"; }"#;
        assert_eq!(
            eval_script(script, MSG).unwrap(),
            vec![Action::FileInto { mailbox: "Sent".into(), flags: vec![] }]
        );
    }

    #[test]
    fn redirect_action() {
        let script = r#"redirect "alice@example.com";"#;
        assert_eq!(
            eval_script(script, MSG).unwrap(),
            vec![Action::Redirect("alice@example.com".into())]
        );
    }

    #[test]
    fn reject_action() {
        let script = r#"reject "policy reject";"#;
        assert_eq!(
            eval_script(script, MSG).unwrap(),
            vec![Action::Reject("policy reject".into())]
        );
    }
}

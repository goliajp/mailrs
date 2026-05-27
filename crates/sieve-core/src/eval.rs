//! RFC 5228 §4 evaluator — walk the AST against a parsed message,
//! emit a list of `Action`s.

use crate::ast::{Action, Argument, Command, MatchType, Test};
use crate::parse::{ParseError, parse_script};

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
        state.actions.push(Action::Keep);
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
}

struct MessageContext<'m> {
    raw: &'m [u8],
}

impl<'m> MessageContext<'m> {
    fn new(raw: &'m [u8]) -> Self {
        Self { raw }
    }

    fn header_values(&self, name: &str) -> Vec<String> {
        let parsed = mailrs_rfc5322::Message::new(self.raw);
        parsed
            .header_all(name)
            .filter_map(|h| {
                h.value_str()
                    .map(|s| s.replace("\r\n ", " ").replace("\r\n\t", " "))
            })
            .collect()
    }

    fn body_size(&self) -> u64 {
        self.raw.len() as u64
    }
}

fn eval_block(
    commands: &[Command],
    ctx: &MessageContext<'_>,
    state: &mut EvalState,
) -> Result<(), EvalError> {
    for cmd in commands {
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
            state.actions.push(Action::Keep);
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
            state.actions.push(Action::FileInto(arg.to_string()));
            state.explicit_action = true;
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
            // RFC 5228 §3.3 — stop terminates evaluation. We
            // emulate this by clearing the remaining commands;
            // since we're inside eval_block, return early.
            // (The caller's loop will continue but no command
            // after stop is processed in this scope.)
            // For simplicity in 0.1 we treat stop as no-op
            // beyond setting explicit_action — the spec-correct
            // behaviour is captured in subsequent slices.
            state.explicit_action = true;
            Ok(())
        }
        "if" => {
            let test = first_test(&cmd.args).ok_or_else(|| EvalError::BadArg {
                cmd: "if".into(),
                detail: "expects test expression".into(),
            })?;
            let matched = eval_test(test, ctx)?;
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
            let matched = eval_test(test, ctx)?;
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

fn eval_test(t: &Test, ctx: &MessageContext<'_>) -> Result<bool, EvalError> {
    match t.name.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        "not" => Ok(!eval_test(&t.children[0], ctx)?),
        "allof" => {
            for c in &t.children {
                if !eval_test(c, ctx)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        "anyof" => {
            for c in &t.children {
                if eval_test(c, ctx)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        "exists" => {
            let names = arg_strings_or_list(&t.args);
            Ok(names.iter().all(|n| !ctx.header_values(n).is_empty()))
        }
        "size" => {
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
            let mode = if t.tags.iter().any(|s| s == "under") {
                "under"
            } else {
                "over"
            };
            Ok(match mode {
                "under" => size < n,
                _ => size > n,
            })
        }
        "header" => {
            let mt = MatchType::from_tags(&t.tags);
            let (names, values) = pair_lists(&t.args);
            for name in &names {
                for hv in ctx.header_values(name) {
                    for needle in &values {
                        if match_string(mt, &hv, needle) {
                            return Ok(true);
                        }
                    }
                }
            }
            Ok(false)
        }
        "address" => {
            let mt = MatchType::from_tags(&t.tags);
            let part = address_part_from_tags(&t.tags);
            let (names, values) = pair_lists(&t.args);
            for name in &names {
                for hv in ctx.header_values(name) {
                    for addr in extract_addresses(&hv) {
                        let scoped = scope_to_part(&addr, part);
                        for needle in &values {
                            if match_string(mt, &scoped, needle) {
                                return Ok(true);
                            }
                        }
                    }
                }
            }
            Ok(false)
        }
        other => Err(EvalError::UnknownTest(other.to_string())),
    }
}

fn first_string(args: &[Argument]) -> Option<&str> {
    args.iter().find_map(|a| match a {
        Argument::String(s) => Some(s.as_str()),
        _ => None,
    })
}

fn first_test(args: &[Argument]) -> Option<&Test> {
    args.iter().find_map(|a| match a {
        Argument::Test(t) => Some(t),
        _ => None,
    })
}

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

fn pair_lists(args: &[Argument]) -> (Vec<String>, Vec<String>) {
    // First non-tag arg is "names" (string-or-list), second is "values".
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

/// Match-type comparison. All comparisons are case-insensitive
/// (RFC 5228 default comparator `i;ascii-casemap`).
fn match_string(mt: MatchType, haystack: &str, needle: &str) -> bool {
    let h = haystack.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    match mt {
        MatchType::Is => h == n,
        MatchType::Contains => h.contains(&n),
        MatchType::Matches => glob_match(&h, &n),
    }
}

/// Tiny glob matcher: `*` matches any sequence, `?` matches one
/// char. ASCII only — sufficient for RFC 5228 `:matches` against
/// header values.
fn glob_match(haystack: &str, pattern: &str) -> bool {
    let h = haystack.as_bytes();
    let p = pattern.as_bytes();
    // recursive memoless implementation; cheap for short patterns
    fn rec(h: &[u8], p: &[u8]) -> bool {
        if p.is_empty() {
            return h.is_empty();
        }
        match p[0] {
            b'*' => {
                // try matching * to empty, then one char, then two, ...
                for i in 0..=h.len() {
                    if rec(&h[i..], &p[1..]) {
                        return true;
                    }
                }
                false
            }
            b'?' => !h.is_empty() && rec(&h[1..], &p[1..]),
            c => !h.is_empty() && h[0] == c && rec(&h[1..], &p[1..]),
        }
    }
    rec(h, p)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddressPart {
    All,
    LocalPart,
    Domain,
}

fn address_part_from_tags(tags: &[String]) -> AddressPart {
    for t in tags {
        match t.as_str() {
            "all" => return AddressPart::All,
            "localpart" | "user" => return AddressPart::LocalPart,
            "domain" => return AddressPart::Domain,
            _ => {}
        }
    }
    AddressPart::All
}

fn scope_to_part(addr: &str, part: AddressPart) -> String {
    match part {
        AddressPart::All => addr.to_string(),
        AddressPart::LocalPart => addr.split_once('@').map(|(l, _)| l.to_string()).unwrap_or_else(|| addr.to_string()),
        AddressPart::Domain => addr.split_once('@').map(|(_, d)| d.to_string()).unwrap_or_default(),
    }
}

/// Naive address extractor: pulls the bare addr-spec(s) out of a
/// raw RFC 5322 address header value. Supports the two common
/// shapes (`alice@example.com` and `Name <alice@example.com>`)
/// plus comma-separated lists. Quoted display names are kept
/// trimmed.
fn extract_addresses(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for piece in value.split(',') {
        let trim = piece.trim();
        if let Some(open) = trim.rfind('<')
            && trim.ends_with('>')
        {
            out.push(trim[open + 1..trim.len() - 1].trim().to_string());
        } else if trim.contains('@') {
            out.push(trim.to_string());
        }
    }
    out
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
        assert_eq!(eval_script("", MSG).unwrap(), vec![Action::Keep]);
    }

    #[test]
    fn explicit_keep() {
        assert_eq!(eval_script("keep;", MSG).unwrap(), vec![Action::Keep]);
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
            vec![Action::FileInto("Junk".into())]
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
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Keep]);
    }

    #[test]
    fn header_contains_substring() {
        let script = r#"if header :contains "Subject" "offer" { fileinto "Ads"; }"#;
        assert_eq!(
            eval_script(script, MSG).unwrap(),
            vec![Action::FileInto("Ads".into())]
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
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Keep]);
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
            vec![Action::FileInto("Spam".into())]
        );
    }

    #[test]
    fn else_branch() {
        let script = r#"
            if header :is "Subject" "no-match" { discard; }
            else { keep; }"#;
        assert_eq!(eval_script(script, MSG).unwrap(), vec![Action::Keep]);
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
            vec![Action::FileInto("Sent".into())]
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

//! RFC 5230 `vacation` action — parser for command arguments and
//! supporting types.
//!
//! The evaluator emits `Action::Vacation(VacationAction)`; the
//! stateful parts (dedup, recipient detection, reply-message
//! build) are the caller's job. This keeps the engine zero-I/O —
//! callers (`mailrs-sieve` wrapper / server inbound pipeline)
//! own all I/O and external state.
//!
//! Differential testing against `sieve-rs` does NOT cover
//! vacation: `sieve-rs` internalises message-building (emits
//! `CreatedMessage` + `SendMessage` events), while this engine
//! surfaces a single `Action::Vacation` value. The abstractions
//! don't line up, so vacation lives in inline unit tests against
//! the RFC 5230 spec instead of the cross-engine corpus.

use crate::ast::{Argument, VacationAction, VacationPeriod};

/// Failure modes when parsing a `vacation` command's argument list.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VacationParseError {
    /// Missing positional `<reason>` string.
    #[error("vacation missing reason string")]
    MissingReason,
    /// More than one positional `<reason>` string.
    #[error("vacation has multiple positional reasons")]
    MultipleReasons,
    /// A `:days` / `:seconds` tag was not followed by a number.
    #[error("vacation tag :{tag} expects a number argument")]
    TagExpectsNumber {
        /// The tag whose argument was missing or wrong type.
        tag: String,
    },
    /// A `:subject` / `:from` / `:handle` tag was not followed by a string.
    #[error("vacation tag :{tag} expects a string argument")]
    TagExpectsString {
        /// The tag whose argument was missing or wrong type.
        tag: String,
    },
    /// `:addresses` was not followed by a string or string-list.
    #[error("vacation :addresses expects a string or string-list")]
    AddressesShape,
    /// Unknown tag at the head of an argument slot.
    #[error("vacation unknown tag :{tag}")]
    UnknownTag {
        /// The unrecognised tag name (without leading colon).
        tag: String,
    },
    /// `:days` and `:seconds` are mutually exclusive but both appeared.
    #[error("vacation cannot combine :days and :seconds")]
    BothDaysAndSeconds,
}

/// Parse the argument vector of a `vacation` command into a
/// `VacationAction`. Caller passes `cmd.args` from the AST.
///
/// Spec source: RFC 5230 §4 (plus `:seconds` from RFC 6131).
pub fn parse_vacation_args(args: &[Argument]) -> Result<VacationAction, VacationParseError> {
    let mut reason: Option<String> = None;
    let mut period: Option<VacationPeriod> = None;
    let mut subject: Option<String> = None;
    let mut from: Option<String> = None;
    let mut addresses: Vec<String> = Vec::new();
    let mut mime = false;
    let mut handle: Option<String> = None;
    let mut saw_days = false;
    let mut saw_seconds = false;

    let mut i = 0usize;
    while i < args.len() {
        match &args[i] {
            Argument::Tag(t) => match t.as_str() {
                "days" => {
                    if saw_seconds {
                        return Err(VacationParseError::BothDaysAndSeconds);
                    }
                    let n = expect_number(args, i + 1, "days")?;
                    period = Some(VacationPeriod::Days(n));
                    saw_days = true;
                    i += 2;
                    continue;
                }
                "seconds" => {
                    if saw_days {
                        return Err(VacationParseError::BothDaysAndSeconds);
                    }
                    let n = expect_number(args, i + 1, "seconds")?;
                    period = Some(VacationPeriod::Seconds(n));
                    saw_seconds = true;
                    i += 2;
                    continue;
                }
                "subject" => {
                    subject = Some(expect_string(args, i + 1, "subject")?);
                    i += 2;
                    continue;
                }
                "from" => {
                    from = Some(expect_string(args, i + 1, "from")?);
                    i += 2;
                    continue;
                }
                "addresses" => {
                    addresses = expect_string_or_list(args, i + 1)?;
                    i += 2;
                    continue;
                }
                "mime" => {
                    mime = true;
                    i += 1;
                    continue;
                }
                "handle" => {
                    handle = Some(expect_string(args, i + 1, "handle")?);
                    i += 2;
                    continue;
                }
                other => {
                    return Err(VacationParseError::UnknownTag {
                        tag: other.to_string(),
                    });
                }
            },
            Argument::String(s) => {
                if reason.is_some() {
                    return Err(VacationParseError::MultipleReasons);
                }
                reason = Some(s.clone());
                i += 1;
            }
            _ => {
                // Numbers / lists / nested tests in non-tag slot are
                // a script bug — treat as "unknown" shape.
                return Err(VacationParseError::AddressesShape);
            }
        }
    }

    let reason = reason.ok_or(VacationParseError::MissingReason)?;
    Ok(VacationAction {
        reason,
        period,
        subject,
        from,
        addresses,
        mime,
        handle,
    })
}

fn expect_number(args: &[Argument], idx: usize, tag: &str) -> Result<u64, VacationParseError> {
    match args.get(idx) {
        Some(Argument::Number(n)) => Ok(*n),
        _ => Err(VacationParseError::TagExpectsNumber {
            tag: tag.to_string(),
        }),
    }
}

fn expect_string(args: &[Argument], idx: usize, tag: &str) -> Result<String, VacationParseError> {
    match args.get(idx) {
        Some(Argument::String(s)) => Ok(s.clone()),
        _ => Err(VacationParseError::TagExpectsString {
            tag: tag.to_string(),
        }),
    }
}

fn expect_string_or_list(args: &[Argument], idx: usize) -> Result<Vec<String>, VacationParseError> {
    match args.get(idx) {
        Some(Argument::String(s)) => Ok(vec![s.clone()]),
        Some(Argument::StringList(v)) => Ok(v.clone()),
        _ => Err(VacationParseError::AddressesShape),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Action;
    use crate::eval::eval_script;

    fn parse(src: &[Argument]) -> VacationAction {
        parse_vacation_args(src).unwrap()
    }

    #[test]
    fn minimal_reason_only() {
        let va = parse(&[Argument::String("I am away".into())]);
        assert_eq!(va.reason, "I am away");
        assert_eq!(va.period, None);
        assert_eq!(va.subject, None);
        assert_eq!(va.from, None);
        assert!(va.addresses.is_empty());
        assert!(!va.mime);
        assert_eq!(va.handle, None);
    }

    #[test]
    fn with_days_and_subject() {
        let va = parse(&[
            Argument::Tag("days".into()),
            Argument::Number(7),
            Argument::Tag("subject".into()),
            Argument::String("Out of office".into()),
            Argument::String("back next week".into()),
        ]);
        assert_eq!(va.period, Some(VacationPeriod::Days(7)));
        assert_eq!(va.subject.as_deref(), Some("Out of office"));
        assert_eq!(va.reason, "back next week");
    }

    #[test]
    fn from_and_addresses_list() {
        let va = parse(&[
            Argument::Tag("from".into()),
            Argument::String("alice@example.com".into()),
            Argument::Tag("addresses".into()),
            Argument::StringList(vec!["alice@a.com".into(), "alice@b.com".into()]),
            Argument::String("away".into()),
        ]);
        assert_eq!(va.from.as_deref(), Some("alice@example.com"));
        assert_eq!(va.addresses, vec!["alice@a.com", "alice@b.com"]);
    }

    #[test]
    fn mime_and_handle() {
        let va = parse(&[
            Argument::Tag("mime".into()),
            Argument::Tag("handle".into()),
            Argument::String("autoreply-2026".into()),
            Argument::String("<html><body>hi</body></html>".into()),
        ]);
        assert!(va.mime);
        assert_eq!(va.handle.as_deref(), Some("autoreply-2026"));
        assert!(va.reason.starts_with("<html>"));
    }

    #[test]
    fn seconds_variant() {
        let va = parse(&[
            Argument::Tag("seconds".into()),
            Argument::Number(3600),
            Argument::String("hourly window".into()),
        ]);
        assert_eq!(va.period, Some(VacationPeriod::Seconds(3600)));
    }

    #[test]
    fn missing_reason_rejected() {
        let err = parse_vacation_args(&[Argument::Tag("days".into()), Argument::Number(7)])
            .unwrap_err();
        assert_eq!(err, VacationParseError::MissingReason);
    }

    #[test]
    fn multiple_reasons_rejected() {
        let err = parse_vacation_args(&[
            Argument::String("first".into()),
            Argument::String("second".into()),
        ])
        .unwrap_err();
        assert_eq!(err, VacationParseError::MultipleReasons);
    }

    #[test]
    fn unknown_tag_rejected() {
        let err = parse_vacation_args(&[
            Argument::Tag("nonesuch".into()),
            Argument::String("reason".into()),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            VacationParseError::UnknownTag {
                tag: "nonesuch".into(),
            }
        );
    }

    #[test]
    fn both_days_and_seconds_rejected() {
        let err = parse_vacation_args(&[
            Argument::Tag("days".into()),
            Argument::Number(7),
            Argument::Tag("seconds".into()),
            Argument::Number(60),
            Argument::String("reason".into()),
        ])
        .unwrap_err();
        assert_eq!(err, VacationParseError::BothDaysAndSeconds);
    }

    // --- Integration tests: full eval_script driving the parser ---

    const MSG: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: hello\r\n\
\r\n\
body\r\n";

    #[test]
    fn vacation_emits_action_and_keeps_implicit_keep() {
        // RFC 5230 §3: vacation does NOT cancel the implicit keep.
        let script = r#"require ["vacation"]; vacation "I am on holiday";"#;
        let actions = eval_script(script, MSG).unwrap();
        assert_eq!(actions.len(), 2, "expected [Vacation, Keep], got {actions:?}");
        match &actions[0] {
            Action::Vacation(va) => {
                assert_eq!(va.reason, "I am on holiday");
                assert_eq!(va.period, None);
            }
            other => panic!("expected Vacation, got {other:?}"),
        }
        assert_eq!(actions[1], Action::Keep);
    }

    #[test]
    fn vacation_with_days_subject_addresses_via_full_eval() {
        let script = r#"require ["vacation"];
            vacation :days 14 :subject "Out" :addresses ["alice@x", "alice@y"] "away";"#;
        let actions = eval_script(script, MSG).unwrap();
        match &actions[0] {
            Action::Vacation(va) => {
                assert_eq!(va.reason, "away");
                assert_eq!(va.period, Some(VacationPeriod::Days(14)));
                assert_eq!(va.subject.as_deref(), Some("Out"));
                assert_eq!(va.addresses, vec!["alice@x", "alice@y"]);
            }
            other => panic!("expected Vacation, got {other:?}"),
        }
    }

    #[test]
    fn vacation_inside_if_with_other_action_no_extra_keep() {
        // fileinto sets explicit_action=true, so no implicit Keep is
        // appended after [Vacation, FileInto].
        let script = r#"require ["vacation", "fileinto"];
            vacation "back tomorrow";
            fileinto "Sent";"#;
        let actions = eval_script(script, MSG).unwrap();
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], Action::Vacation(_)));
        assert_eq!(actions[1], Action::FileInto("Sent".into()));
    }
}

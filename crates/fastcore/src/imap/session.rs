//! Per-connection IMAP session — state machine + command dispatch.
//!
//! Structure follows RFC 3501 §3: NotAuthenticated → Authenticated →
//! Selected → Logout. LOGIN flips NotAuthenticated → Authenticated;
//! SELECT / EXAMINE flip Authenticated → Selected; LOGOUT / CLOSE
//! terminate the connection. Unknown / unsupported commands answer
//! `BAD`, unauthenticated commands answer `NO`.
//!
//! The session leans heavily on `mailrs-imap-proto` (parser +
//! formatter) and `mailrs-imap-codec` (framing) so we don't reinvent
//! the wire format. Backend calls go through [`crate::imap::backend`].

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use mailrs_imap_codec::{ImapCodec, ImapInput};
use mailrs_imap_proto::{
    ImapCommand, format_bad, format_bye, format_capability, format_no, format_ok, parse_command,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use super::backend::{self, ImapMessage, MailboxInfo};
use crate::FastcoreState;

use super::fetch::*;
use super::mailbox::*;
use super::query::*;

/// Session state per RFC 3501 §3.
#[derive(Debug, Clone)]
pub(super) enum State {
    NotAuthed,
    Authed {
        user: String,
    },
    Selected {
        user: String,
        mailbox: MailboxInfo,
        messages: Vec<ImapMessage>,
        read_only: bool,
    },
}

impl State {
    pub(super) fn user(&self) -> Option<&str> {
        match self {
            State::NotAuthed => None,
            State::Authed { user } | State::Selected { user, .. } => Some(user),
        }
    }
}

/// Entry point — takes a plaintext connection and drives it to
/// completion. STARTTLS transitions happen at the listener layer
/// (this loop doesn't own the socket type after upgrade).
pub async fn run<S>(state: Arc<FastcoreState>, io: S)
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let mut framed = Framed::new(io, ImapCodec::new());
    // Greeting per RFC 3501 §7.5. Sent before the client says anything.
    let greeting = format!(
        "* OK [CAPABILITY IMAP4rev1 IDLE STARTTLS AUTH=PLAIN NAMESPACE ENABLE SORT QUOTA CONDSTORE QRESYNC] {} ready\r\n",
        state
            .mailbox
            .store_ref()
            .get(b"hostname")
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_else(|| "mailrs".into())
    );
    if framed.send(greeting.into_bytes()).await.is_err() {
        return;
    }

    let mut session = State::NotAuthed;
    // ENABLE QRESYNC is connection-scoped (RFC 7162 §3.2.5)
    let mut qresync = false;
    while let Some(frame) = framed.next().await {
        let Ok(input) = frame else { return };
        let line = match input {
            ImapInput::Line(s) => s,
            ImapInput::LiteralData(_) => {
                // Standalone literal outside APPEND flow — ignore.
                continue;
            }
        };
        let parsed = match parse_command(line.trim_end()) {
            Ok(cmd) => cmd,
            Err(e) => {
                let _ = framed
                    .send(format_bad("*", &format!("parse: {e}")).into_bytes())
                    .await;
                continue;
            }
        };
        let is_logout = matches!(parsed.command, ImapCommand::Logout);
        let tag = parsed.tag;
        let responses = dispatch(
            &state,
            &mut session,
            &tag,
            parsed.command,
            &mut framed,
            &mut qresync,
        )
        .await;
        for r in responses {
            if framed.send(r.into_bytes()).await.is_err() {
                return;
            }
        }
        if is_logout {
            return;
        }
    }
}

async fn dispatch<S>(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    cmd: ImapCommand,
    framed: &mut Framed<S, ImapCodec>,
    qresync: &mut bool,
) -> Vec<String>
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    match cmd {
        ImapCommand::Capability => vec![
            format_capability(&[
                "IMAP4rev1",
                "AUTH=PLAIN",
                "IDLE",
                "NAMESPACE",
                "ENABLE",
                "UNSELECT",
                "MOVE",
                "SPECIAL-USE",
                "SORT",
                "QUOTA",
                "CONDSTORE",
                "QRESYNC",
            ]),
            format_ok(tag, "CAPABILITY completed"),
        ],
        ImapCommand::Noop => vec![format_ok(tag, "NOOP completed")],
        ImapCommand::Logout => vec![
            format_bye("mailrs logging out"),
            format_ok(tag, "LOGOUT completed"),
        ],
        ImapCommand::Login { username, password } => {
            login(state, session, tag, &username, &password)
        }
        ImapCommand::List {
            reference: _,
            pattern,
        } => list_response(state, session, tag, &pattern),
        ImapCommand::Select { mailbox } => select(state, session, tag, &mailbox, false, *qresync),
        ImapCommand::Examine { mailbox } => select(state, session, tag, &mailbox, true, *qresync),
        ImapCommand::Enable(caps) => {
            let mut enabled = Vec::new();
            for c in &caps {
                let up = c.to_ascii_uppercase();
                if up == "QRESYNC" || up == "CONDSTORE" {
                    if up == "QRESYNC" {
                        *qresync = true;
                    }
                    enabled.push(up);
                }
            }
            vec![
                format!("* ENABLED {}\r\n", enabled.join(" ")),
                format_ok(tag, "ENABLE completed"),
            ]
        }
        ImapCommand::Close => close(session, tag),
        ImapCommand::Fetch {
            sequence,
            attributes,
        } => fetch_response(session, tag, &sequence, &attributes, false),
        ImapCommand::Uid { subcommand } => match *subcommand {
            ImapCommand::Fetch {
                sequence,
                attributes,
            } => fetch_response(session, tag, &sequence, &attributes, true),
            ImapCommand::Store {
                sequence,
                action,
                flags,
            } => store_response(state, session, tag, &sequence, &action, &flags, true),
            ImapCommand::Search { criteria } => search_response(session, tag, &criteria, true),
            ImapCommand::Sort {
                criteria,
                charset: _,
                search_criteria,
            } => sort_response(session, tag, &criteria, &search_criteria, true),
            ImapCommand::Copy { sequence, mailbox } => {
                copy_response(state, session, tag, &sequence, &mailbox, false, true)
            }
            ImapCommand::Move { sequence, mailbox } => {
                copy_response(state, session, tag, &sequence, &mailbox, true, true)
            }
            _ => vec![format_bad(tag, "UID subcommand not supported")],
        },
        ImapCommand::Store {
            sequence,
            action,
            flags,
        } => store_response(state, session, tag, &sequence, &action, &flags, false),
        ImapCommand::Search { criteria } => search_response(session, tag, &criteria, false),
        ImapCommand::Expunge => expunge(state, session, tag, *qresync),
        ImapCommand::Copy { sequence, mailbox } => {
            copy_response(state, session, tag, &sequence, &mailbox, false, false)
        }
        ImapCommand::Move { sequence, mailbox } => {
            copy_response(state, session, tag, &sequence, &mailbox, true, false)
        }
        ImapCommand::Append {
            mailbox,
            flags: _flags,
            literal_size,
        } => append_flow(state, session, tag, framed, &mailbox, literal_size).await,
        ImapCommand::Idle => idle_flow(state, session, tag, framed).await,
        ImapCommand::Sort {
            criteria,
            charset: _,
            search_criteria,
        } => sort_response(session, tag, &criteria, &search_criteria, false),
        ImapCommand::GetQuota { quotaroot: _ } => quota_response(state, session, tag),
        ImapCommand::GetQuotaRoot { mailbox } => {
            let mut out = vec![format!("* QUOTAROOT \"{mailbox}\" \"\"\r\n")];
            out.extend(quota_response(state, session, tag));
            out
        }
        _ => vec![format_bad(tag, "command not implemented")],
    }
}

fn login(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    username: &str,
    password: &str,
) -> Vec<String> {
    if !matches!(session, State::NotAuthed) {
        return vec![format_bad(tag, "already authenticated")];
    }
    if backend::verify_password(state, username, password) {
        *session = State::Authed {
            user: username.to_string(),
        };
        vec![format_ok(tag, "LOGIN completed")]
    } else {
        vec![format_no(tag, "invalid credentials")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_maildir::Flag;

    use crate::imap::fetch::{flags_to_imap, parse_imap_flags};

    #[test]
    fn wildcard_matches_star() {
        assert!(match_wildcard("INBOX", "*"));
        assert!(match_wildcard("Sent", "*"));
        assert!(match_wildcard("Work.Client", "*"));
    }

    #[test]
    fn wildcard_matches_percent() {
        // Maildir has no hierarchy separator in our IMAP naming, so
        // % and * both collapse to "anything" per RFC 3501 §6.3.8.
        assert!(match_wildcard("INBOX", "%"));
        assert!(match_wildcard("Work.Client", "%"));
    }

    #[test]
    fn wildcard_matches_prefix() {
        assert!(match_wildcard("Sent", "S*"));
        assert!(!match_wildcard("Draft", "S*"));
    }

    #[test]
    fn parse_flags_reads_backslash_names() {
        let f = parse_imap_flags("(\\Seen \\Flagged)");
        assert!(f.contains(&Flag::Seen));
        assert!(f.contains(&Flag::Flagged));
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn flags_to_imap_serialises_known() {
        let s = flags_to_imap(&[Flag::Seen, Flag::Flagged]);
        assert_eq!(s, "\\Seen \\Flagged");
    }
}

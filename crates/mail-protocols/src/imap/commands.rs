//! Command dispatch: which handler answers each parsed command, and the
//! state checks every handler shares.

use std::sync::Arc;

use mailrs_imap_proto::{ImapCommand, format_bad, format_bye, format_no, format_ok};
use tokio::io::{AsyncRead, AsyncWrite};

use super::append::append_flow;
use super::fetch::fetch_response;
use super::folders::{fixed, subscription};
use super::idle::idle_flow;
use super::mailbox::{close, list_response, select, unselect};
use super::query::{copy_response, quota_response, search_response, sort_response};
use super::session::{Io, State, no};
use super::status::status_response;
use super::store::{expunge, store_response};
use crate::backend::{Account, Directory, Protocol};

/// Commands that change mail, which a read-only account refuses whatever
/// state the session is in.
fn changes_mail(cmd: &ImapCommand) -> bool {
    match cmd {
        ImapCommand::Store { .. }
        | ImapCommand::Copy { .. }
        | ImapCommand::Move { .. }
        | ImapCommand::Append { .. }
        | ImapCommand::Expunge
        | ImapCommand::Create { .. }
        | ImapCommand::Delete { .. }
        | ImapCommand::Rename { .. } => true,
        ImapCommand::Uid { subcommand } => matches!(
            **subcommand,
            ImapCommand::Store { .. } | ImapCommand::Copy { .. } | ImapCommand::Move { .. }
        ),
        _ => false,
    }
}

pub(super) async fn dispatch<D, S>(
    dir: &D,
    session: &mut State<D::Account>,
    tag: &str,
    cmd: ImapCommand,
    framed: &mut Io<S>,
    qresync: &mut bool,
) -> Vec<String>
where
    D: Directory,
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    if changes_mail(&cmd)
        && let Some(reason) = session.account().and_then(|a| a.read_only())
    {
        return vec![format_no(tag, reason)];
    }
    match cmd {
        ImapCommand::Noop => vec![format_ok(tag, "NOOP completed")],
        ImapCommand::Logout => vec![
            format_bye("mailrs logging out"),
            format_ok(tag, "LOGOUT completed"),
        ],
        ImapCommand::Login { username, password } => {
            login(dir, session, tag, &username, &password).await
        }
        ImapCommand::List { pattern, .. } => list_response(session, tag, &pattern, "LIST").await,
        ImapCommand::Lsub { pattern, .. } => list_response(session, tag, &pattern, "LSUB").await,
        ImapCommand::Create { .. } | ImapCommand::Delete { .. } | ImapCommand::Rename { .. } => {
            fixed(session, tag)
        }
        ImapCommand::Subscribe { mailbox } => {
            subscription(session, tag, &mailbox, "SUBSCRIBE").await
        }
        ImapCommand::Unsubscribe { mailbox } => {
            subscription(session, tag, &mailbox, "UNSUBSCRIBE").await
        }
        ImapCommand::Status { mailbox, items } => {
            status_response(session, tag, &mailbox, &items).await
        }
        ImapCommand::Select { mailbox } => select(session, tag, &mailbox, false, *qresync).await,
        ImapCommand::Examine { mailbox } => select(session, tag, &mailbox, true, *qresync).await,
        ImapCommand::Enable(caps) => enable(tag, &caps, qresync),
        ImapCommand::Close => close(session, tag, *qresync).await,
        ImapCommand::Unselect => unselect(session, tag),
        ImapCommand::Uid { subcommand } => uid(session, tag, *subcommand).await,
        ImapCommand::Expunge => expunge(session, tag, *qresync).await,
        ImapCommand::Append {
            mailbox,
            flags,
            literal_size,
        } => {
            append_flow(
                session,
                tag,
                framed,
                &mailbox,
                flags.as_deref(),
                literal_size,
            )
            .await
        }
        ImapCommand::Idle => idle_flow(session, tag, framed).await,
        ImapCommand::GetQuota { .. } => quota_response(session, tag).await,
        ImapCommand::GetQuotaRoot { mailbox } => {
            let mut out = vec![format!("* QUOTAROOT \"{mailbox}\" \"\"\r\n")];
            out.extend(quota_response(session, tag).await);
            out
        }
        cmd => sequenced(session, tag, cmd, false).await,
    }
}

/// UID FETCH / STORE / SEARCH / SORT / COPY / MOVE.
async fn uid<A: Account>(session: &mut State<A>, tag: &str, cmd: ImapCommand) -> Vec<String> {
    match cmd {
        ImapCommand::Fetch { .. }
        | ImapCommand::Store { .. }
        | ImapCommand::Search { .. }
        | ImapCommand::Sort { .. }
        | ImapCommand::Copy { .. }
        | ImapCommand::Move { .. } => sequenced(session, tag, cmd, true).await,
        _ => vec![format_bad(tag, "UID subcommand not supported")],
    }
}

/// The commands that address messages by sequence number or, under UID,
/// by uid.
async fn sequenced<A: Account>(
    session: &mut State<A>,
    tag: &str,
    cmd: ImapCommand,
    by_uid: bool,
) -> Vec<String> {
    match cmd {
        ImapCommand::Fetch {
            sequence,
            attributes,
        } => fetch_response(session, tag, &sequence, &attributes, by_uid).await,
        ImapCommand::Store {
            sequence,
            action,
            flags,
        } => store_response(session, tag, &sequence, &action, &flags, by_uid).await,
        ImapCommand::Search { criteria } => search_response(session, tag, &criteria, by_uid).await,
        ImapCommand::Sort {
            criteria,
            search_criteria,
            ..
        } => sort_response(session, tag, &criteria, &search_criteria, by_uid).await,
        ImapCommand::Copy { sequence, mailbox } => {
            copy_response(session, tag, &sequence, &mailbox, false, by_uid).await
        }
        ImapCommand::Move { sequence, mailbox } => {
            copy_response(session, tag, &sequence, &mailbox, true, by_uid).await
        }
        _ => vec![format_bad(tag, "command not implemented")],
    }
}

fn enable(tag: &str, caps: &[String], qresync: &mut bool) -> Vec<String> {
    let mut enabled = Vec::new();
    for c in caps {
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

async fn login<D: Directory>(
    dir: &D,
    session: &mut State<D::Account>,
    tag: &str,
    username: &str,
    password: &str,
) -> Vec<String> {
    if !matches!(session, State::NotAuthed) {
        return vec![format_bad(tag, "already authenticated")];
    }
    match dir.login(username, password, Protocol::Imap).await {
        Ok(Some(account)) => {
            *session = State::Authed {
                account: Arc::new(account),
            };
            vec![format_ok(tag, "LOGIN completed")]
        }
        Ok(None) => vec![format_no(tag, "invalid credentials")],
        Err(e) => vec![no(tag, "LOGIN", &e)],
    }
}

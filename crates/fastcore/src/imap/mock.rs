//! IMAP over a mock namespace: `{ns}@{mock domain}` logs in with the
//! namespace's SMTP token and reads the namespace's mail as a read-only
//! INBOX.
//!
//! Only the mail addressed to the namespace is listed. Mail its sandbox
//! sent is kept in the same namespace, but an app that reads its INBOX
//! and answers what it finds would otherwise answer its own outbound
//! mail.
//!
//! [`intercept`] answers every command whose generic handler would read
//! or write a real account's maildir; the rest (FETCH, SEARCH, SORT,
//! CLOSE, …) run unchanged over the messages put in the session here.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use mailrs_core_sidestate::families::mock as store;
use mailrs_imap_codec::{ImapCodec, ImapInput};
use mailrs_imap_proto::{
    ImapCommand, format_bad, format_exists, format_flags, format_list, format_no, format_ok,
    format_recent,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use super::backend::{ImapMessage, MailboxInfo};
use super::mailbox::match_wildcard;
use super::session::State;
use crate::FastcoreState;

const READ_ONLY: &str = "[READ-ONLY] mock mailboxes are read-only";

/// The answer to `cmd` when it is a mock login or comes from a mock
/// session; `None` hands it to the generic handler.
pub(super) async fn intercept<S>(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    cmd: &ImapCommand,
    framed: &mut Framed<S, ImapCodec>,
) -> Option<Vec<String>>
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    if let ImapCommand::Login { username, password } = cmd
        && store::is_mock(username)
    {
        return Some(login(state, session, tag, username, password).await);
    }
    let ns = store::parse(session.user()?)?.namespace;
    let out = match cmd {
        ImapCommand::List { pattern, .. } => list(tag, pattern),
        ImapCommand::Select { mailbox } | ImapCommand::Examine { mailbox } => {
            select(state, session, tag, ns, mailbox).await
        }
        ImapCommand::Idle => idle(tag, framed).await,
        ImapCommand::Store { .. }
        | ImapCommand::Copy { .. }
        | ImapCommand::Move { .. }
        | ImapCommand::Append { .. }
        | ImapCommand::Expunge
        | ImapCommand::Create { .. }
        | ImapCommand::Delete { .. }
        | ImapCommand::Rename { .. } => vec![format_no(tag, READ_ONLY)],
        ImapCommand::Uid { subcommand } => match **subcommand {
            ImapCommand::Store { .. } | ImapCommand::Copy { .. } | ImapCommand::Move { .. } => {
                vec![format_no(tag, READ_ONLY)]
            }
            _ => return None,
        },
        _ => return None,
    };
    Some(out)
}

/// Only the namespace's token is checked: a username on the mock domain
/// never reaches the account store.
async fn login(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    username: &str,
    password: &str,
) -> Vec<String> {
    if !matches!(session, State::NotAuthed) {
        return vec![format_bad(tag, "already authenticated")];
    }
    let Some(addr) = store::parse(username).filter(|a| a.tag.is_empty()) else {
        return vec![format_no(tag, "invalid credentials")];
    };
    let st = state.clone();
    let ns = addr.namespace.clone();
    let token = password.to_string();
    let verified = tokio::task::spawn_blocking(move || {
        let mut conn = st.net_conn()?;
        store::verify_smtp_token(&mut conn, &ns, &token).ok()
    })
    .await
    .ok()
    .flatten();
    match verified {
        Some(true) => {
            *session = State::Authed {
                user: format!("{}@{}", addr.namespace, store::domain()),
            };
            vec![format_ok(tag, "LOGIN completed")]
        }
        Some(false) => vec![format_no(tag, "invalid credentials")],
        None => vec![format_no(tag, "[UNAVAILABLE] mock store unreachable")],
    }
}

fn list(tag: &str, pattern: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(2);
    if match_wildcard("INBOX", pattern) {
        out.push(format_list("\\HasNoChildren", "/", "INBOX"));
    }
    out.push(format_ok(tag, "LIST completed"));
    out
}

/// SELECT and EXAMINE both open the INBOX read-only. A QRESYNC parameter
/// list is ignored: nothing is ever expunged by a client, so there is no
/// delta to report.
async fn select(
    state: &Arc<FastcoreState>,
    session: &mut State,
    tag: &str,
    ns: String,
    mailbox: &str,
) -> Vec<String> {
    let name = mailbox.split_once(" (").map_or(mailbox, |(n, _)| n);
    if !name.trim().trim_matches('"').eq_ignore_ascii_case("INBOX") {
        return vec![format_no(
            tag,
            "[NONEXISTENT] a mock namespace has only INBOX",
        )];
    }
    let Some(user) = session.user().map(str::to_string) else {
        return vec![format_no(tag, "not authenticated")];
    };
    let st = state.clone();
    let loaded = tokio::task::spawn_blocking(move || load(&st, &ns).map(|v| (ns, v)))
        .await
        .ok()
        .flatten();
    let Some((ns, (uidvalidity, uidnext, messages))) = loaded else {
        return vec![format_no(tag, "[UNAVAILABLE] mock store unreachable")];
    };
    let out = vec![
        format_flags(&[]),
        format_exists(messages.len() as u32),
        format_recent(0),
        format!("* OK [UIDVALIDITY {uidvalidity}] Version 1\r\n"),
        format!("* OK [UIDNEXT {uidnext}] Predicted next UID\r\n"),
        "* OK [HIGHESTMODSEQ 1] Modseq\r\n".to_string(),
        "* OK [PERMANENTFLAGS ()] Read-only\r\n".to_string(),
        format_ok(tag, "[READ-ONLY] SELECT completed"),
    ];
    *session = State::Selected {
        user,
        mailbox: MailboxInfo {
            name: "INBOX".into(),
            path: store::root().join(&ns),
        },
        messages,
        read_only: true,
    };
    out
}

/// UIDVALIDITY and the inbound messages of `ns`, ascending by uid.
/// UIDVALIDITY is the creation second, so a namespace deleted and
/// created again under the same name invalidates what clients cached.
fn load(state: &FastcoreState, ns: &str) -> Option<(u32, u32, Vec<ImapMessage>)> {
    let mut conn = state.net_conn()?;
    let info = store::get_namespace(&mut conn, ns).ok()??;
    let all = store::Filter {
        limit: usize::MAX,
        ..Default::default()
    };
    let mut metas = store::list_messages(&mut conn, ns, &all).ok()?;
    // uid 0: stored before uids were handed out, not addressable over IMAP
    metas.retain(|m| m.direction == store::Direction::In && m.uid != 0);
    metas.sort_by_key(|m| m.uid);
    let messages = metas
        .into_iter()
        .enumerate()
        // `size` is the length of the bytes the file was written with
        .map(|(i, m)| ImapMessage {
            uid: m.uid,
            seqno: i as u32 + 1,
            path: store::message_path(store::root(), ns, &m.id),
            flags: Vec::new(),
            internal_date: m.received_at / 1000,
            size: m.size as u64,
            modseq: 1,
        })
        .collect();
    let uidvalidity = u32::try_from(info.created_at / 1000)
        .unwrap_or(u32::MAX)
        .max(1);
    // from the counter, not the newest message: UIDNEXT must not go down
    // when that message is deleted or expires
    let uidnext = store::last_uid(&mut conn, ns).ok()?.saturating_add(1);
    Some((uidvalidity, uidnext, messages))
}

/// Waits for DONE without reporting anything; new mail shows on the next
/// SELECT.
async fn idle<S>(tag: &str, framed: &mut Framed<S, ImapCodec>) -> Vec<String>
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    if framed.send(b"+ idling\r\n".to_vec()).await.is_err() {
        return Vec::new();
    }
    loop {
        match framed.next().await {
            Some(Ok(ImapInput::Line(l))) if l.trim().eq_ignore_ascii_case("DONE") => break,
            Some(Ok(_)) => continue,
            _ => return Vec::new(),
        }
    }
    vec![format_ok(tag, "IDLE terminated")]
}

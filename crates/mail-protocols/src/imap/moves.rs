//! COPY and MOVE (RFC 6851).

use mailrs_imap_proto::{format_no, format_ok};

use super::query::expand_sequence;
use super::session::{State, no};
use super::store::{announce, refresh};
use crate::backend::Account;

pub(super) async fn copy_response<A: Account>(
    session: &mut State<A>,
    tag: &str,
    sequence: &str,
    mailbox: &str,
    by_uid: bool,
) -> Vec<String> {
    let State::Selected {
        account,
        mailbox: from,
        messages,
        ..
    } = &*session
    else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    let dest = match account.mailbox(mailbox).await {
        Ok(Some(d)) => d,
        Ok(None) => return vec![format_no(tag, "no such destination")],
        Err(e) => return vec![no(tag, "COPY", &e)],
    };
    let (account, from) = (account.clone(), from.clone());
    let mut failed = None;
    for msg in expand_sequence(sequence, messages, by_uid) {
        if let Err(e) = account.copy(&from, &msg, &dest).await {
            failed = Some(e);
            break;
        }
    }
    let refreshed = refresh(session).await;
    match (failed, refreshed) {
        (Some(e), _) | (None, Err(e)) => vec![no(tag, "COPY", &e)],
        (None, Ok(())) => vec![format_ok(tag, "COPY completed")],
    }
}

/// The messages moved leave the selected mailbox as an EXPUNGE does, so
/// the client is told the same way, those moved before a failure too.
pub(super) async fn move_response<A: Account>(
    session: &mut State<A>,
    tag: &str,
    sequence: &str,
    mailbox: &str,
    by_uid: bool,
    qresync: bool,
) -> Vec<String> {
    let State::Selected {
        account,
        mailbox: from,
        messages,
        ..
    } = &*session
    else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    let dest = match account.mailbox(mailbox).await {
        Ok(Some(d)) => d,
        Ok(None) => return vec![format_no(tag, "no such destination")],
        Err(e) => return vec![no(tag, "MOVE", &e)],
    };
    let (account, from) = (account.clone(), from.clone());
    let mut moved = Vec::new();
    let mut failed = None;
    for msg in expand_sequence(sequence, messages, by_uid) {
        match account.move_to(&from, &msg, &dest).await {
            Ok(()) => moved.push(msg),
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    let mut out = announce(&moved, qresync);
    let refreshed = refresh(session).await;
    out.push(match (failed, refreshed) {
        (Some(e), _) | (None, Err(e)) => no(tag, "MOVE", &e),
        (None, Ok(())) => format_ok(tag, "MOVE completed"),
    });
    out
}

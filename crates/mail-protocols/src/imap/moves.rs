//! COPY and MOVE (RFC 6851), each naming where the messages went
//! (UIDPLUS, RFC 4315).

use mailrs_imap_proto::{format_no, format_ok};

use super::query::expand_sequence;
use super::session::{State, no};
use super::store::{announce, refresh};
use crate::backend::{Account, Filed};

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
    let mut copied = Vec::new();
    let mut failed = None;
    for msg in expand_sequence(sequence, messages, by_uid) {
        match account.copy(&from, &msg, &dest).await {
            Ok(filed) => copied.push((msg.uid, filed)),
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    let refreshed = refresh(session).await;
    match (failed, refreshed) {
        (Some(e), _) | (None, Err(e)) => vec![no(tag, "COPY", &e)],
        (None, Ok(())) => match copyuid(&copied) {
            Some(code) => vec![format_ok(tag, &format!("{code} COPY completed"))],
            None => vec![format_ok(tag, "COPY completed")],
        },
    }
}

/// `[COPYUID uidvalidity source-uids destination-uids]` (RFC 4315 §3),
/// the two lists in the same order; `None` when nothing was filed.
fn copyuid(filed: &[(u32, Filed)]) -> Option<String> {
    let (_, first) = filed.first()?;
    let list = |uids: Vec<u32>| {
        let words: Vec<String> = uids.iter().map(u32::to_string).collect();
        words.join(",")
    };
    let from = list(filed.iter().map(|(uid, _)| *uid).collect());
    let to = list(filed.iter().map(|(_, f)| f.uid).collect());
    Some(format!("[COPYUID {} {from} {to}]", first.uidvalidity))
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
    let mut filed = Vec::new();
    let mut failed = None;
    for msg in expand_sequence(sequence, messages, by_uid) {
        match account.move_to(&from, &msg, &dest).await {
            Ok(f) => {
                filed.push((msg.uid, f));
                moved.push(msg);
            }
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    // RFC 6851 §4.3: COPYUID in an untagged OK, before the expunges
    let mut out: Vec<String> = copyuid(&filed)
        .map(|code| format!("* OK {code} Moved\r\n"))
        .into_iter()
        .collect();
    out.extend(announce(&moved, qresync));
    let refreshed = refresh(session).await;
    out.push(match (failed, refreshed) {
        (Some(e), _) | (None, Err(e)) => no(tag, "MOVE", &e),
        (None, Ok(())) => format_ok(tag, "MOVE completed"),
    });
    out
}

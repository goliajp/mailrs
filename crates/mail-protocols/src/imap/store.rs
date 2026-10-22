//! STORE and EXPUNGE — the two commands that change messages in place.

use mailrs_imap_proto::{format_fetch, format_no, format_ok};

use super::flags::{flags_to_imap, parse_imap_flags};
use super::query::expand_sequence;
use super::session::{State, no};
use crate::backend::{Account, Error, Flag};

/// Re-list the selected mailbox after a change.
pub(super) async fn refresh<A: Account>(session: &mut State<A>) -> Result<(), Error> {
    if let State::Selected {
        account,
        mailbox,
        messages,
        ..
    } = session
    {
        *messages = account.messages(mailbox).await?;
    }
    Ok(())
}

/// `(UNCHANGEDSINCE n)` (RFC 7162) and the real action and flag list.
/// The proto splits STORE args as (sequence, action, flags); with the
/// modifier present the pieces arrive as action="(UNCHANGEDSINCE",
/// flags="<n>) +FLAGS (...)".
fn unchangedsince(action: &str, flags: &str) -> (Option<u64>, String, String) {
    if !action.to_ascii_uppercase().starts_with("(UNCHANGEDSINCE") {
        return (None, action.to_string(), flags.to_string());
    }
    match flags.split_once(')') {
        Some((n, rest)) => {
            let modseq = n.trim().parse::<u64>().ok();
            let rest = rest.trim();
            match rest.split_once(' ') {
                Some((act, flg)) => (modseq, act.to_string(), flg.to_string()),
                None => (modseq, rest.to_string(), String::new()),
            }
        }
        None => (None, action.to_string(), flags.to_string()),
    }
}

fn merge(current: &[Flag], action: &str, given: &[Flag]) -> Vec<Flag> {
    let mut merged: Vec<Flag> = match action {
        a if a.starts_with("+FLAGS") => {
            let mut m = current.to_vec();
            for f in given {
                if !m.contains(f) {
                    m.push(*f);
                }
            }
            m
        }
        a if a.starts_with("-FLAGS") => current
            .iter()
            .copied()
            .filter(|f| !given.contains(f))
            .collect(),
        _ => given.to_vec(),
    };
    merged.sort_by_key(|f| *f as u32);
    merged.dedup();
    merged
}

pub(super) async fn store_response<A: Account>(
    session: &mut State<A>,
    tag: &str,
    sequence: &str,
    action: &str,
    flags: &str,
    by_uid: bool,
) -> Vec<String> {
    let State::Selected {
        account,
        mailbox,
        messages,
        read_only,
    } = &*session
    else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    if *read_only {
        return vec![format_no(tag, "mailbox is read-only")];
    }
    let (account, mailbox) = (account.clone(), mailbox.clone());
    let ids = expand_sequence(sequence, messages, by_uid);
    let (unchangedsince, action, flags) = unchangedsince(action, flags);
    let given = parse_imap_flags(&flags);
    let action = action.to_uppercase();
    let mut modified: Vec<u32> = Vec::new();
    let mut out = Vec::with_capacity(ids.len() + 1);
    for msg in ids {
        if let Some(since) = unchangedsince
            && msg.modseq > since
        {
            // changed behind the client's back — refuse this one
            modified.push(if by_uid { msg.uid } else { msg.seqno });
            continue;
        }
        let merged = merge(&msg.flags, &action, &given);
        if let Err(e) = account.store_flags(&mailbox, &msg, &merged).await {
            out.push(match refresh(session).await {
                Ok(()) => no(tag, "STORE", &e),
                Err(r) => no(tag, "STORE", &r),
            });
            return out;
        }
        if !action.ends_with(".SILENT") {
            let flags_str = flags_to_imap(&merged);
            out.push(format_fetch(
                msg.seqno,
                &[("FLAGS".into(), format!("({flags_str})"))],
            ));
        }
    }
    if let Err(e) = refresh(session).await {
        out.push(no(tag, "STORE", &e));
        return out;
    }
    if modified.is_empty() {
        out.push(format_ok(tag, "STORE completed"));
    } else {
        let list = modified
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!(
            "{tag} OK [MODIFIED {list}] Conditional STORE failed for some messages\r\n"
        ));
    }
    out
}

pub(super) async fn expunge<A: Account>(
    session: &mut State<A>,
    tag: &str,
    qresync: bool,
) -> Vec<String> {
    let State::Selected {
        account,
        mailbox,
        messages,
        read_only,
    } = &*session
    else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    if *read_only {
        return vec![format_no(tag, "mailbox is read-only")];
    }
    // RFC 3501 requires * EXPUNGE for each deleted message in descending
    // order, so the seqnos announced stay correct
    let mut doomed: Vec<_> = messages
        .iter()
        .filter(|m| m.flags.contains(&Flag::Trashed))
        .cloned()
        .collect();
    doomed.sort_unstable_by_key(|m| std::cmp::Reverse(m.seqno));
    let gone = account.expunge(mailbox, &doomed).await;
    let mut out = Vec::new();
    match qresync {
        // QRESYNC-enabled sessions get VANISHED instead of seqno EXPUNGE
        true if !gone.removed.is_empty() => {
            let mut uids = gone.removed.clone();
            uids.sort_unstable();
            let list = uids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            out.push(format!("* VANISHED {list}\r\n"));
        }
        true => {}
        false => {
            for uid in &gone.removed {
                if let Some(m) = doomed.iter().find(|m| m.uid == *uid) {
                    out.push(format!("* {} EXPUNGE\r\n", m.seqno));
                }
            }
        }
    }
    let refreshed = refresh(session).await;
    match (gone.failed, refreshed) {
        (Some(e), _) | (None, Err(e)) => out.push(no(tag, "EXPUNGE", &e)),
        (None, Ok(())) => out.push(format_ok(tag, "EXPUNGE completed")),
    }
    out
}

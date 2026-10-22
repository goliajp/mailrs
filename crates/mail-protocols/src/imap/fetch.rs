//! FETCH: the items of each addressed message, with the CONDSTORE
//! `CHANGEDSINCE` modifier and the `MODSEQ` item (RFC 7162).

use mailrs_imap_proto::{format_fetch, format_no, format_ok};

use super::fetch_items::{Item, answer, needs_bytes, requested, sets_seen};
use super::flags::flags_to_imap;
use super::query::expand_sequence;
use super::session::{State, no};
use super::store::{merge, refresh};
use crate::backend::{Account, Flag};

/// `FETCH 1:* (FLAGS) (CHANGEDSINCE 42)`
fn changed_since(attributes: &str) -> Option<u64> {
    let upper = attributes.to_uppercase();
    let pos = upper.find("CHANGEDSINCE")?;
    attributes
        .get(pos + "CHANGEDSINCE".len()..)
        .unwrap_or("")
        .split_whitespace()
        .next()
        .and_then(|t| t.trim_end_matches(')').parse::<u64>().ok())
}

pub(super) async fn fetch_response<A: Account>(
    session: &mut State<A>,
    tag: &str,
    sequence: &str,
    attributes: &str,
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
    let (account, mailbox, read_only) = (account.clone(), mailbox.clone(), *read_only);
    let ids = expand_sequence(sequence, messages, by_uid);
    let changedsince = changed_since(attributes);
    let modseq_asked = attributes.to_uppercase().contains("MODSEQ");
    let mut out = Vec::with_capacity(ids.len() + 1);
    let mut stored = false;
    for mut msg in ids {
        if let Some(since) = changedsince
            && msg.modseq <= since
        {
            continue;
        }
        let wanted = requested(attributes, by_uid);
        let bytes = match needs_bytes(&wanted) {
            true => match account.read(&msg).await {
                Ok(bytes) => bytes,
                Err(e) => {
                    out.push(no(tag, "FETCH", &e));
                    return out;
                }
            },
            false => None,
        };
        // the same write a STORE +FLAGS (\Seen) makes, after the read:
        // on a Maildir it renames the file the bytes came from
        let seen_now = !read_only && sets_seen(&wanted) && !msg.flags.contains(&Flag::Seen);
        if seen_now {
            let flags = merge(&msg.flags, "+FLAGS", &[Flag::Seen]);
            if let Err(e) = account.store_flags(&mailbox, &msg, &flags).await {
                out.push(match refresh(session).await {
                    Ok(()) => no(tag, "FETCH", &e),
                    Err(r) => no(tag, "FETCH", &r),
                });
                return out;
            }
            msg.flags = flags;
            stored = true;
        }
        let flags_asked = wanted.contains(&Item::Flags);
        let mut items = answer(&msg, wanted, bytes.as_deref());
        if seen_now && !flags_asked {
            items.push(("FLAGS".into(), format!("({})", flags_to_imap(&msg.flags))));
        }
        // MODSEQ item: explicit request or implied by CHANGEDSINCE
        if changedsince.is_some() || modseq_asked {
            items.push(("MODSEQ".into(), format!("({})", msg.modseq)));
        }
        out.push(format_fetch(msg.seqno, &items));
    }
    if stored && let Err(e) = refresh(session).await {
        out.push(no(tag, "FETCH", &e));
        return out;
    }
    out.push(format_ok(tag, "FETCH completed"));
    out
}

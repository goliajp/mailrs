//! STATUS (RFC 3501 §6.3.10), with HIGHESTMODSEQ (RFC 7162 §3.1.4).

use mailrs_imap_proto::{format_bad, format_no, format_ok};

use super::session::{State, no};
use crate::backend::{Account, Flag, Selected};

#[derive(Clone, Copy)]
enum Item {
    Messages,
    Recent,
    UidNext,
    UidValidity,
    Unseen,
    HighestModseq,
}

impl Item {
    fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_uppercase().as_str() {
            "MESSAGES" => Self::Messages,
            "RECENT" => Self::Recent,
            "UIDNEXT" => Self::UidNext,
            "UIDVALIDITY" => Self::UidValidity,
            "UNSEEN" => Self::Unseen,
            "HIGHESTMODSEQ" => Self::HighestModseq,
            _ => return None,
        })
    }

    fn answer<K>(self, sel: &Selected<K>) -> String {
        match self {
            Self::Messages => format!("MESSAGES {}", sel.messages.len()),
            Self::Recent => format!("RECENT {}", sel.recent),
            Self::UidNext => format!("UIDNEXT {}", sel.uidnext),
            Self::UidValidity => format!("UIDVALIDITY {}", sel.uidvalidity),
            Self::Unseen => {
                let unseen = sel.messages.iter();
                let n = unseen.filter(|m| !m.flags.contains(&Flag::Seen)).count();
                format!("UNSEEN {n}")
            }
            Self::HighestModseq => format!("HIGHESTMODSEQ {}", sel.highest_modseq),
        }
    }
}

/// The items of `(MESSAGES UNSEEN …)`, or the first word that is not one.
fn parse_items(list: &str) -> Result<Vec<Item>, &str> {
    let inner = list.trim().trim_start_matches('(').trim_end_matches(')');
    inner
        .split_whitespace()
        .map(|w| Item::parse(w).ok_or(w))
        .collect()
}

pub(super) async fn status_response<A: Account>(
    session: &State<A>,
    tag: &str,
    mailbox: &str,
    items: &str,
) -> Vec<String> {
    let Some(account) = session.account() else {
        return vec![format_no(tag, "not authenticated")];
    };
    let wanted = match parse_items(items) {
        Ok(w) if !w.is_empty() => w,
        Ok(_) => return vec![format_bad(tag, "STATUS needs at least one item")],
        Err(word) => return vec![format_bad(tag, &format!("unknown STATUS item {word}"))],
    };
    let mb = match account.mailbox(mailbox).await {
        Ok(Some(mb)) => mb,
        Ok(None) => return vec![format_no(tag, "[NONEXISTENT] no such mailbox")],
        Err(e) => return vec![no(tag, "STATUS", &e)],
    };
    let sel = match account.select(&mb).await {
        Ok(s) => s,
        Err(e) => return vec![no(tag, "STATUS", &e)],
    };
    let answers: Vec<String> = wanted.iter().map(|i| i.answer(&sel)).collect();
    vec![
        format!(
            "* STATUS {} ({})\r\n",
            account.mailbox_name(&mb),
            answers.join(" ")
        ),
        format_ok(tag, "STATUS completed"),
    ]
}

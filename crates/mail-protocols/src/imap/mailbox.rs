//! Mailbox-level commands: LIST, SELECT / EXAMINE, CLOSE.

use mailrs_imap_proto::{
    format_bad, format_exists, format_flags, format_list, format_no, format_ok, format_recent,
    special_use_flag,
};

use super::flags::flags_to_imap;
use super::session::{State, no};
use crate::backend::{Account, Selected};

pub(super) async fn list_response<A: Account>(
    session: &State<A>,
    tag: &str,
    pattern: &str,
) -> Vec<String> {
    let Some(account) = session.account() else {
        return vec![format_no(tag, "not authenticated")];
    };
    let names = match account.mailbox_names().await {
        Ok(n) => n,
        Err(e) => return vec![no(tag, "LIST", &e)],
    };
    let mut out = Vec::with_capacity(names.len() + 1);
    for name in names {
        if match_wildcard(&name, pattern) {
            // RFC 6154 §5.1: the SPECIAL-USE attribute lets MUAs map the
            // folder to their own Junk / Sent / Drafts / Trash / Archive
            let flags = format!("\\HasNoChildren{}", special_use_flag(&name));
            out.push(format_list(&flags, "/", &name));
        }
    }
    out.push(format_ok(tag, "LIST completed"));
    out
}

/// IMAP wildcard match — `*` recursive, `%` single-level (RFC 3501
/// §6.3.8 says `%` is one hierarchy level but there is no hierarchy
/// separator in these names, so `%` and `*` collapse). An empty pattern
/// matches everything, matching most clients' initial LIST probe.
pub(super) fn match_wildcard(name: &str, pattern: &str) -> bool {
    if pattern.is_empty() || pattern == "*" || pattern == "%" {
        return true;
    }
    // split on `*` / `%`; each literal segment must appear in order
    let mut segments: Vec<&str> = Vec::new();
    let mut cur = 0;
    let bytes = pattern.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'*' || *b == b'%' {
            if i > cur {
                segments.push(&pattern[cur..i]);
            }
            segments.push("*");
            cur = i + 1;
        }
    }
    if cur < bytes.len() {
        segments.push(&pattern[cur..]);
    }
    let name_lower = name.to_lowercase();
    let mut pos = 0;
    let mut requires_prefix = !pattern.starts_with('*') && !pattern.starts_with('%');
    let requires_suffix = !pattern.ends_with('*') && !pattern.ends_with('%');
    let last_idx = segments.len().saturating_sub(1);
    for (i, seg) in segments.iter().enumerate() {
        if *seg == "*" {
            continue;
        }
        let seg_l = seg.to_lowercase();
        let found = match i == 0 && requires_prefix {
            true => name_lower.starts_with(&seg_l).then_some(0),
            false => name_lower[pos..].find(&seg_l).map(|p| p + pos),
        };
        let Some(f) = found else { return false };
        pos = f + seg_l.len();
        requires_prefix = false;
        if i == last_idx && requires_suffix && pos != name_lower.len() {
            return false;
        }
    }
    true
}

/// `SELECT "INBOX" (QRESYNC (uidvalidity modseq))` — the proto hands the
/// whole tail as the mailbox arg; split the optional QRESYNC parameter
/// list off (RFC 7162 §3.2.5).
fn qresync_params(mailbox: &str) -> (&str, Option<(u32, u64)>) {
    let Some((name, params)) = mailbox.split_once(" (") else {
        return (mailbox, None);
    };
    let name = name.trim().trim_matches('"');
    let up = params.to_ascii_uppercase();
    let parsed = up.strip_prefix("QRESYNC (").and_then(|rest| {
        let rest = rest.trim_end_matches(')');
        let mut it = rest.split_whitespace();
        let uv = it.next()?.parse::<u32>().ok()?;
        let ms = it.next()?.parse::<u64>().ok()?;
        Some((uv, ms))
    });
    (name, parsed)
}

fn select_lines<K>(sel: &Selected<K>, read_only: bool, writable: bool) -> Vec<String> {
    let flags_line = match writable {
        true => format_flags(&["\\Seen", "\\Answered", "\\Flagged", "\\Deleted", "\\Draft"]),
        false => format_flags(&[]),
    };
    let permanent = match read_only {
        true => "* OK [PERMANENTFLAGS ()] Read-only\r\n".to_string(),
        false => {
            "* OK [PERMANENTFLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)] Limited\r\n"
                .to_string()
        }
    };
    vec![
        flags_line,
        format_exists(sel.messages.len() as u32),
        format_recent(sel.recent),
        format!("* OK [UIDVALIDITY {}] Version 1\r\n", sel.uidvalidity),
        format!("* OK [UIDNEXT {}] Predicted next UID\r\n", sel.uidnext),
        format!("* OK [HIGHESTMODSEQ {}] Modseq\r\n", sel.highest_modseq),
        permanent,
    ]
}

/// The untagged QRESYNC delta: uids vanished and messages changed since
/// the modseq the client last saw.
async fn qresync_delta<A: Account>(
    account: &A,
    mailbox: &A::Mailbox,
    sel: &Selected<A::Key>,
    client_ms: u64,
) -> Result<Vec<String>, crate::backend::Error> {
    let mut lines = Vec::new();
    let vanished = account.vanished_since(mailbox, client_ms).await?;
    if !vanished.is_empty() {
        let list = vanished
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        lines.push(format!("* VANISHED (EARLIER) {list}\r\n"));
    }
    for m in sel.messages.iter().filter(|m| m.modseq > client_ms) {
        lines.push(format!(
            "* {} FETCH (UID {} FLAGS ({}) MODSEQ ({}))\r\n",
            m.seqno,
            m.uid,
            flags_to_imap(&m.flags),
            m.modseq
        ));
    }
    Ok(lines)
}

pub(super) async fn select<A: Account>(
    session: &mut State<A>,
    tag: &str,
    mailbox: &str,
    examine: bool,
    qresync: bool,
) -> Vec<String> {
    let Some(account) = session.account().cloned() else {
        return vec![format_no(tag, "not authenticated")];
    };
    let verb = if examine { "EXAMINE" } else { "SELECT" };
    let (name, params) = qresync_params(mailbox);
    let mb = match account.mailbox(name).await {
        Ok(Some(mb)) => mb,
        Ok(None) => return vec![format_no(tag, "no such mailbox")],
        Err(e) => return vec![no(tag, verb, &e)],
    };
    let sel = match account.select(&mb).await {
        Ok(s) => s,
        Err(e) => return vec![no(tag, verb, &e)],
    };
    let writable = account.read_only().is_none();
    let read_only = examine || !writable;
    let mut out = select_lines(&sel, read_only, writable);
    // only when the client ENABLEd it, supplied params, and its cached
    // uidvalidity still matches
    if qresync
        && let Some((client_uv, client_ms)) = params
        && client_uv == sel.uidvalidity
    {
        match qresync_delta(&*account, &mb, &sel, client_ms).await {
            Ok(lines) => out.extend(lines),
            Err(e) => return vec![no(tag, verb, &e)],
        }
    }
    out.push(match read_only {
        true => format_ok(tag, &format!("[READ-ONLY] {verb} completed")),
        false => format_ok(tag, "[READ-WRITE] SELECT completed"),
    });
    *session = State::Selected {
        account,
        mailbox: mb,
        messages: sel.messages,
        read_only,
    };
    out
}

pub(super) fn close<A: Account>(session: &mut State<A>, tag: &str) -> Vec<String> {
    let State::Selected { account, .. } = session else {
        return vec![format_bad(tag, "not in SELECTED state")];
    };
    *session = State::Authed {
        account: account.clone(),
    };
    vec![format_ok(tag, "CLOSE completed")]
}

#[cfg(test)]
mod tests {
    use super::match_wildcard;

    #[test]
    fn wildcard_matches_star() {
        assert!(match_wildcard("INBOX", "*"));
        assert!(match_wildcard("Sent", "*"));
        assert!(match_wildcard("Work.Client", "*"));
    }

    #[test]
    fn wildcard_matches_percent() {
        assert!(match_wildcard("INBOX", "%"));
        assert!(match_wildcard("Work.Client", "%"));
    }

    #[test]
    fn wildcard_matches_prefix() {
        assert!(match_wildcard("Sent", "S*"));
        assert!(!match_wildcard("Draft", "S*"));
    }
}

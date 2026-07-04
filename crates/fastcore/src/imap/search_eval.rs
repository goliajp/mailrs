//! SEARCH / SORT evaluation over the scanned mailbox (G3.4 / G3.5).
//!
//! Works directly on the maildir files the session already listed:
//! header keys read the first 16 KB lazily (one read per message per
//! command, cached), TEXT/BODY read the whole file. Fine at the scale
//! an IMAP SELECT already tolerates — the scan itself stat()s every
//! file anyway.

use std::cell::OnceCell;

use mailrs_imap_proto::SearchKey;
use mailrs_maildir::Flag;

use super::backend::ImapMessage;

/// Lazy per-message content cache for one SEARCH/SORT pass.
pub struct MsgView<'a> {
    msg: &'a ImapMessage,
    head: OnceCell<Vec<u8>>,
    full: OnceCell<Vec<u8>>,
}

impl<'a> MsgView<'a> {
    /// Wrap a message for evaluation.
    pub fn new(msg: &'a ImapMessage) -> Self {
        Self {
            msg,
            head: OnceCell::new(),
            full: OnceCell::new(),
        }
    }

    fn head(&self) -> &[u8] {
        self.head.get_or_init(|| {
            std::fs::read(&self.msg.path)
                .map(|b| {
                    let cut = b.len().min(16 * 1024);
                    b[..cut].to_vec()
                })
                .unwrap_or_default()
        })
    }

    fn full(&self) -> &[u8] {
        self.full
            .get_or_init(|| std::fs::read(&self.msg.path).unwrap_or_default())
    }

    /// Case-insensitive unfolded header lookup.
    pub fn header(&self, name: &str) -> Option<String> {
        let text = String::from_utf8_lossy(self.head());
        let head_end = text.find("\r\n\r\n").or_else(|| text.find("\n\n"));
        let head = &text[..head_end.unwrap_or(text.len())];
        let mut lines: Vec<String> = Vec::new();
        let mut cur = String::new();
        for line in head.split('\n') {
            let line = line.trim_end_matches('\r');
            if line.starts_with(' ') || line.starts_with('\t') {
                cur.push(' ');
                cur.push_str(line.trim_start());
            } else {
                if !cur.is_empty() {
                    lines.push(std::mem::take(&mut cur));
                }
                cur.push_str(line);
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        let want = name.to_ascii_lowercase();
        for l in &lines {
            if let Some((n, v)) = l.split_once(':')
                && n.trim().to_ascii_lowercase() == want
            {
                return Some(v.trim().to_string());
            }
        }
        None
    }

    fn header_contains(&self, name: &str, needle: &str) -> bool {
        self.header(name)
            .map(|v| v.to_lowercase().contains(&needle.to_lowercase()))
            .unwrap_or(false)
    }

    fn has_flag(&self, f: Flag) -> bool {
        self.msg.flags.contains(&f)
    }
}

/// Evaluate one search key against one message.
pub fn matches(key: &SearchKey, v: &MsgView<'_>) -> bool {
    match key {
        SearchKey::All => true,
        SearchKey::Seen => v.has_flag(Flag::Seen),
        SearchKey::Unseen => !v.has_flag(Flag::Seen),
        SearchKey::Flagged => v.has_flag(Flag::Flagged),
        SearchKey::Unflagged => !v.has_flag(Flag::Flagged),
        SearchKey::Answered => v.has_flag(Flag::Replied),
        SearchKey::Unanswered => !v.has_flag(Flag::Replied),
        SearchKey::Deleted => v.has_flag(Flag::Trashed),
        SearchKey::Undeleted => !v.has_flag(Flag::Trashed),
        SearchKey::Draft => v.has_flag(Flag::Draft),
        SearchKey::Undraft => !v.has_flag(Flag::Draft),
        // fresh scans have no session-recent tracking — approximate
        // RECENT/NEW as "unseen", the practical client intent
        SearchKey::Recent => !v.has_flag(Flag::Seen),
        SearchKey::From(s) => v.header_contains("From", s),
        SearchKey::To(s) => v.header_contains("To", s) || v.header_contains("Cc", s),
        SearchKey::Subject(s) => v.header_contains("Subject", s),
        SearchKey::Header(name, s) => {
            if s.is_empty() {
                v.header(name).is_some()
            } else {
                v.header_contains(name, s)
            }
        }
        SearchKey::Text(s) => {
            let hay = String::from_utf8_lossy(v.full()).to_lowercase();
            hay.contains(&s.to_lowercase())
        }
        SearchKey::Body(s) => {
            let raw = v.full();
            let split = raw
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| p + 4)
                .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|p| p + 2))
                .unwrap_or(0);
            let hay = String::from_utf8_lossy(&raw[split..]).to_lowercase();
            hay.contains(&s.to_lowercase())
        }
        SearchKey::Since(ts) => v.msg.internal_date >= *ts,
        SearchKey::Before(ts) => v.msg.internal_date < *ts,
        SearchKey::On(ts) => v.msg.internal_date >= *ts && v.msg.internal_date < *ts + 86_400,
        SearchKey::Larger(n) => v.msg.size > *n as u64,
        SearchKey::Smaller(n) => v.msg.size < *n as u64,
        SearchKey::Uid(spec) => uid_in_set(v.msg.uid, spec),
        SearchKey::Or(a, b) => matches(a, v) || matches(b, v),
        SearchKey::Not(k) => !matches(k, v),
        SearchKey::And(ks) => ks.iter().all(|k| matches(k, v)),
    }
}

/// Filter messages by an AND-list of search keys.
pub fn filter<'a>(keys: &[SearchKey], msgs: &'a [ImapMessage]) -> Vec<&'a ImapMessage> {
    msgs.iter()
        .filter(|m| {
            let v = MsgView::new(m);
            keys.iter().all(|k| matches(k, &v))
        })
        .collect()
}

/// `1:5,8,*` style uid-set membership (subset of RFC 3501 seq-set —
/// `*` means "any", ranges + singles supported).
fn uid_in_set(uid: u32, spec: &str) -> bool {
    for part in spec.split(',') {
        let part = part.trim();
        if part == "*" {
            return true;
        }
        if let Some((a, b)) = part.split_once(':') {
            let lo = if a == "*" {
                1
            } else {
                a.parse().unwrap_or(u32::MAX)
            };
            let hi = if b == "*" {
                u32::MAX
            } else {
                b.parse().unwrap_or(0)
            };
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            if uid >= lo && uid <= hi {
                return true;
            }
        } else if part.parse::<u32>() == Ok(uid) {
            return true;
        }
    }
    false
}

/// One RFC 5256 sort criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortCrit {
    /// Internal (arrival) date.
    Arrival,
    /// Date header (approximated with internal date — the drain stamps
    /// internal_date from the Date header when parseable).
    Date,
    /// Subject header, case-folded, `Re:`-stripped.
    Subject,
    /// From header, case-folded.
    From,
    /// To header, case-folded.
    To,
    /// Message size.
    Size,
}

/// Parse `REVERSE DATE SUBJECT` → (reversed?, criteria list).
pub fn parse_sort_criteria(s: &str) -> (bool, Vec<SortCrit>) {
    let mut reverse = false;
    let mut crits = Vec::new();
    for tok in s.split_whitespace() {
        match tok.to_ascii_uppercase().as_str() {
            "REVERSE" => reverse = true,
            "ARRIVAL" => crits.push(SortCrit::Arrival),
            "DATE" => crits.push(SortCrit::Date),
            "SUBJECT" => crits.push(SortCrit::Subject),
            "FROM" => crits.push(SortCrit::From),
            "TO" => crits.push(SortCrit::To),
            "SIZE" => crits.push(SortCrit::Size),
            _ => {}
        }
    }
    if crits.is_empty() {
        crits.push(SortCrit::Arrival);
    }
    (reverse, crits)
}

fn base_subject(s: &str) -> String {
    let mut t = s.trim();
    loop {
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("re:") || lower.starts_with("fw:") {
            t = t[3..].trim_start();
        } else if lower.starts_with("fwd:") {
            t = t[4..].trim_start();
        } else {
            break;
        }
    }
    t.to_lowercase()
}

/// Sort filtered messages per criteria; ties fall back to uid order.
pub fn sort<'a>(
    mut msgs: Vec<&'a ImapMessage>,
    reverse: bool,
    crits: &[SortCrit],
) -> Vec<&'a ImapMessage> {
    msgs.sort_by(|a, b| {
        for c in crits {
            let ord = match c {
                SortCrit::Arrival | SortCrit::Date => a.internal_date.cmp(&b.internal_date),
                SortCrit::Size => a.size.cmp(&b.size),
                SortCrit::Subject => {
                    let (va, vb) = (MsgView::new(a), MsgView::new(b));
                    base_subject(&va.header("Subject").unwrap_or_default())
                        .cmp(&base_subject(&vb.header("Subject").unwrap_or_default()))
                }
                SortCrit::From => {
                    let (va, vb) = (MsgView::new(a), MsgView::new(b));
                    va.header("From")
                        .unwrap_or_default()
                        .to_lowercase()
                        .cmp(&vb.header("From").unwrap_or_default().to_lowercase())
                }
                SortCrit::To => {
                    let (va, vb) = (MsgView::new(a), MsgView::new(b));
                    va.header("To")
                        .unwrap_or_default()
                        .to_lowercase()
                        .cmp(&vb.header("To").unwrap_or_default().to_lowercase())
                }
            };
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        a.uid.cmp(&b.uid)
    });
    if reverse {
        msgs.reverse();
    }
    msgs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn msg(uid: u32, date: i64, size: u64, path: &str) -> ImapMessage {
        ImapMessage {
            uid,
            seqno: uid,
            path: PathBuf::from(path),
            flags: vec![],
            internal_date: date,
            size,
            modseq: 1,
        }
    }

    fn write_tmp(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn uid_set_membership() {
        assert!(uid_in_set(5, "1:10"));
        assert!(uid_in_set(5, "5"));
        assert!(uid_in_set(5, "1,3,5"));
        assert!(!uid_in_set(5, "1,3,7:9"));
        assert!(uid_in_set(5, "*"));
        assert!(uid_in_set(5, "3:*"));
    }

    #[test]
    fn or_not_and_evaluation() {
        let tmp = tempfile::tempdir().unwrap();
        let p = write_tmp(
            tmp.path(),
            "m1",
            "From: alice@x.y\r\nSubject: hello world\r\n\r\nbody text",
        );
        let m = msg(1, 1000, 40, p.to_str().unwrap());
        let v = MsgView::new(&m);
        use SearchKey::*;
        assert!(matches(
            &Or(Box::new(From("bob".into())), Box::new(From("alice".into()))),
            &v
        ));
        assert!(matches(&Not(Box::new(From("bob".into()))), &v));
        assert!(matches(
            &And(vec![Subject("hello".into()), Text("body".into())]),
            &v
        ));
        assert!(!matches(
            &And(vec![Subject("hello".into()), Text("nope".into())]),
            &v
        ));
        assert!(matches(&Header("Subject".into(), "world".into()), &v));
        assert!(matches(&Larger(10), &v));
        assert!(!matches(&Smaller(10), &v));
    }

    #[test]
    fn sort_by_subject_strips_re_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let p1 = write_tmp(tmp.path(), "m1", "Subject: Re: banana\r\n\r\n.");
        let p2 = write_tmp(tmp.path(), "m2", "Subject: apple\r\n\r\n.");
        let m1 = msg(1, 10, 1, p1.to_str().unwrap());
        let m2 = msg(2, 20, 1, p2.to_str().unwrap());
        let sorted = sort(vec![&m1, &m2], false, &[SortCrit::Subject]);
        assert_eq!(sorted[0].uid, 2, "apple before (Re:) banana");
        let (rev, crits) = parse_sort_criteria("REVERSE SUBJECT");
        assert!(rev);
        assert_eq!(crits, vec![SortCrit::Subject]);
    }
}

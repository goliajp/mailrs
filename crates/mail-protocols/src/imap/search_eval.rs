//! SEARCH / SORT evaluation over a selected mailbox.
//!
//! Header keys look at the first 16 KB of a message, TEXT / BODY at all
//! of it. The caller reads the bytes once per message per command, and
//! only when a key or sort criterion looks inside the message.

use mailrs_imap_proto::SearchKey;

use crate::backend::{Flag, Message};

/// One message and its bytes, for one SEARCH/SORT pass.
pub struct MsgView<'a, K> {
    msg: &'a Message<K>,
    raw: &'a [u8],
}

impl<'a, K> MsgView<'a, K> {
    /// `raw` may be empty when no key reads the content.
    pub fn new(msg: &'a Message<K>, raw: &'a [u8]) -> Self {
        Self { msg, raw }
    }

    pub fn msg(&self) -> &'a Message<K> {
        self.msg
    }

    fn head(&self) -> &[u8] {
        &self.raw[..self.raw.len().min(16 * 1024)]
    }

    fn full(&self) -> &[u8] {
        self.raw
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
pub fn matches<K>(key: &SearchKey, v: &MsgView<'_, K>) -> bool {
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

/// Whether any of `keys` looks inside the message.
pub fn reads_content(keys: &[SearchKey]) -> bool {
    keys.iter().any(reads)
}

fn reads(key: &SearchKey) -> bool {
    match key {
        SearchKey::From(_)
        | SearchKey::To(_)
        | SearchKey::Subject(_)
        | SearchKey::Header(..)
        | SearchKey::Text(_)
        | SearchKey::Body(_) => true,
        SearchKey::Or(a, b) => reads(a) || reads(b),
        SearchKey::Not(k) => reads(k),
        SearchKey::And(ks) => reads_content(ks),
        _ => false,
    }
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

/// Whether ordering by `crits` reads headers.
pub fn sorts_on_headers(crits: &[SortCrit]) -> bool {
    crits
        .iter()
        .any(|c| matches!(c, SortCrit::Subject | SortCrit::From | SortCrit::To))
}

/// Sort filtered messages per criteria; ties fall back to uid order.
pub fn sort<'a, K>(
    mut msgs: Vec<MsgView<'a, K>>,
    reverse: bool,
    crits: &[SortCrit],
) -> Vec<MsgView<'a, K>> {
    msgs.sort_by(|va, vb| {
        let (a, b) = (va.msg, vb.msg);
        for c in crits {
            let ord = match c {
                SortCrit::Arrival | SortCrit::Date => a.internal_date.cmp(&b.internal_date),
                SortCrit::Size => a.size.cmp(&b.size),
                SortCrit::Subject => base_subject(&va.header("Subject").unwrap_or_default())
                    .cmp(&base_subject(&vb.header("Subject").unwrap_or_default())),
                SortCrit::From => va
                    .header("From")
                    .unwrap_or_default()
                    .to_lowercase()
                    .cmp(&vb.header("From").unwrap_or_default().to_lowercase()),
                SortCrit::To => va
                    .header("To")
                    .unwrap_or_default()
                    .to_lowercase()
                    .cmp(&vb.header("To").unwrap_or_default().to_lowercase()),
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

    fn msg(uid: u32, date: i64, size: u64) -> Message<()> {
        Message {
            uid,
            seqno: uid,
            flags: vec![],
            internal_date: date,
            size,
            modseq: 1,
            key: (),
        }
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
        let raw = b"From: alice@x.y\r\nSubject: hello world\r\n\r\nbody text";
        let m = msg(1, 1000, 40);
        let v = MsgView::new(&m, raw);
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
    fn only_keys_on_the_content_read_it() {
        use SearchKey::*;
        assert!(!reads_content(&[Seen, Uid("1:*".into()), Larger(3)]));
        assert!(reads_content(&[Not(Box::new(Or(
            Box::new(Seen),
            Box::new(Body("x".into()))
        )))]));
    }

    #[test]
    fn sort_by_subject_strips_re_prefix() {
        let (m1, m2) = (msg(1, 10, 1), msg(2, 20, 1));
        let views = vec![
            MsgView::new(&m1, b"Subject: Re: banana\r\n\r\n."),
            MsgView::new(&m2, b"Subject: apple\r\n\r\n."),
        ];
        let sorted = sort(views, false, &[SortCrit::Subject]);
        assert_eq!(sorted[0].msg().uid, 2, "apple before (Re:) banana");
        let (rev, crits) = parse_sort_criteria("REVERSE SUBJECT");
        assert!(rev);
        assert_eq!(crits, vec![SortCrit::Subject]);
    }
}

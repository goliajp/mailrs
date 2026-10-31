//! Search operators: what a query like `from:alice has:attachment invoice`
//! asks for, and whether a conversation answers it.
//!
//! Both cores parse with `Query::parse` and decide with `Query::admits`;
//! only finding the candidates and reading their facts is theirs. A rule
//! is a saved query, so what a rule matches on delivery is exactly what
//! the same text finds in search.

use chrono::NaiveDate;

/// A parsed search.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    /// The words that are not operators, for the text indexes. Empty when
    /// the query is operators only.
    pub text: String,
    /// The operators, each possibly negated with a leading `-`.
    pub conds: Vec<(bool, Cond)>,
    /// Where to look, from `in:`. `None` keeps the view the search was
    /// made from.
    pub within: Option<Within>,
}

/// One operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cond {
    /// `from:` — a sender contains this, case-insensitively.
    From(String),
    /// `to:` — a To or Cc recipient contains this.
    To(String),
    /// `subject:` — the subject contains this.
    Subject(String),
    /// `has:attachment`.
    HasAttachment,
    /// `after:` — latest mail at or after this instant (epoch seconds).
    After(i64),
    /// `before:` — latest mail before this instant (epoch seconds).
    Before(i64),
    /// `label:` — carries a label with this name, case-insensitively.
    Label(String),
    /// `is:`.
    Is(Is),
}

/// The states `is:` can ask about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Is {
    /// Has an unread message.
    Unread,
    /// Every message read.
    Read,
    /// Starred.
    Starred,
    /// Pinned.
    Pinned,
}

/// The places `in:` can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Within {
    /// The Inbox.
    Inbox,
    /// Archived.
    Archive,
    /// Junk.
    Junk,
    /// The Trash.
    Trash,
    /// Mail the user sent.
    Sent,
    /// Starred.
    Starred,
    /// Everywhere, Junk and Trash included.
    Anywhere,
}

/// Where a conversation is, for `in:`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Place {
    /// Archived by this reader.
    pub archived: bool,
    /// In this reader's Trash.
    pub trashed: bool,
    /// In the Junk bucket.
    pub junk: bool,
    /// Has mail this reader sent.
    pub has_sent: bool,
    /// Every message in it was sent by this reader.
    pub sent_only: bool,
    /// Starred by this reader.
    pub starred: bool,
}

impl Within {
    /// Whether a conversation in `place` is somewhere this names.
    pub fn admits(self, p: Place) -> bool {
        let live = !p.trashed;
        match self {
            Within::Inbox => live && !p.archived && !p.junk && !p.sent_only,
            Within::Archive => live && p.archived && !p.junk,
            Within::Junk => live && p.junk,
            Within::Trash => p.trashed,
            Within::Sent => live && p.has_sent,
            Within::Starred => live && p.starred,
            Within::Anywhere => true,
        }
    }
}

/// What a conversation's row says, for the operators that can be decided
/// without reading its mail.
#[derive(Debug, Clone, Copy)]
pub struct ThreadFacts<'a> {
    /// The conversation's subject.
    pub subject: &'a str,
    /// Its senders as the row keeps them.
    pub senders: &'a str,
    /// Epoch seconds of the latest message.
    pub latest_date: i64,
    /// Unread messages in it.
    pub unread_count: i64,
    /// Starred by this reader.
    pub starred: bool,
    /// Pinned by this reader.
    pub pinned: bool,
    /// The names of the labels on it.
    pub labels: &'a [String],
}

/// One message of a conversation, for the operators the row cannot answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageFacts {
    /// The From header.
    pub sender: String,
    /// The To and Cc recipients.
    pub recipients: String,
    /// The subject.
    pub subject: String,
    /// Attachments, or `None` when never counted (mail from before the
    /// count was recorded, not yet backfilled).
    pub attachments: Option<u32>,
}

impl Query {
    /// Parse a query. Anything that is not a well-formed operator is text,
    /// as Gmail treats it: `foo:bar` or `after:2026-13-01` is searched for
    /// as written rather than refused.
    pub fn parse(input: &str) -> Query {
        let mut q = Query::default();
        let mut text: Vec<String> = Vec::new();
        for token in tokens(input) {
            match operator(&token) {
                Some(Op::Cond(neg, cond)) => q.conds.push((neg, cond)),
                Some(Op::Within(w)) => q.within = Some(w),
                None => text.push(token.unquoted),
            }
        }
        q.text = text.join(" ");
        q
    }

    /// Whether any operator needs the conversation's messages read.
    pub fn needs_messages(&self) -> bool {
        self.conds.iter().any(|(_, c)| {
            matches!(
                c,
                Cond::From(_) | Cond::To(_) | Cond::Subject(_) | Cond::HasAttachment
            )
        })
    }

    /// Whether the query has anything besides text, `in:` included: `in:junk`
    /// alone lists Junk.
    pub fn has_operators(&self) -> bool {
        !self.conds.is_empty() || self.within.is_some()
    }

    /// Whether a conversation answers every operator. `messages` is called
    /// at most once, and only when the row alone cannot decide.
    pub fn admits<F>(&self, thread: &ThreadFacts<'_>, messages: F) -> bool
    where
        F: FnOnce() -> Vec<MessageFacts>,
    {
        let mut lazy = Lazy {
            load: Some(messages),
            loaded: Vec::new(),
        };
        self.conds
            .iter()
            .all(|(negated, cond)| holds(cond, thread, &mut lazy) != *negated)
    }
}

/// A conversation's messages, read the first time an operator asks.
struct Lazy<F> {
    load: Option<F>,
    loaded: Vec<MessageFacts>,
}

impl<F: FnOnce() -> Vec<MessageFacts>> Lazy<F> {
    fn get(&mut self) -> &[MessageFacts] {
        if let Some(load) = self.load.take() {
            self.loaded = load();
        }
        &self.loaded
    }
}

fn holds<F: FnOnce() -> Vec<MessageFacts>>(
    cond: &Cond,
    t: &ThreadFacts<'_>,
    msgs: &mut Lazy<F>,
) -> bool {
    match cond {
        Cond::From(x) => {
            contains(t.senders, x) || msgs.get().iter().any(|m| contains(&m.sender, x))
        }
        Cond::To(x) => msgs.get().iter().any(|m| contains(&m.recipients, x)),
        Cond::Subject(x) => {
            contains(t.subject, x) || msgs.get().iter().any(|m| contains(&m.subject, x))
        }
        Cond::HasAttachment => msgs.get().iter().any(|m| m.attachments.unwrap_or(0) > 0),
        Cond::After(at) => t.latest_date >= *at,
        Cond::Before(at) => t.latest_date < *at,
        Cond::Label(name) => t.labels.iter().any(|l| l.eq_ignore_ascii_case(name)),
        Cond::Is(Is::Unread) => t.unread_count > 0,
        Cond::Is(Is::Read) => t.unread_count == 0,
        Cond::Is(Is::Starred) => t.starred,
        Cond::Is(Is::Pinned) => t.pinned,
    }
}

fn contains(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

struct Token {
    /// As typed, quotes included, for recognising `key:value`.
    raw: String,
    /// With quotes removed, for text and values.
    unquoted: String,
}

fn tokens(input: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut raw = String::new();
    let mut unquoted = String::new();
    let mut quoted = false;
    for ch in input.chars() {
        if ch == '"' {
            quoted = !quoted;
            raw.push(ch);
            continue;
        }
        if ch.is_whitespace() && !quoted {
            if !raw.is_empty() {
                out.push(Token {
                    raw: std::mem::take(&mut raw),
                    unquoted: std::mem::take(&mut unquoted),
                });
            }
            continue;
        }
        raw.push(ch);
        unquoted.push(ch);
    }
    if !raw.is_empty() {
        out.push(Token { raw, unquoted });
    }
    out
}

enum Op {
    Cond(bool, Cond),
    Within(Within),
}

fn operator(token: &Token) -> Option<Op> {
    if token.raw.starts_with('"') {
        return None;
    }
    let (negated, body) = match token.unquoted.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, token.unquoted.as_str()),
    };
    let (key, value) = body.split_once(':')?;
    if value.is_empty() {
        return None;
    }
    let cond = match key.to_ascii_lowercase().as_str() {
        "from" => Cond::From(value.to_string()),
        "to" => Cond::To(value.to_string()),
        "subject" => Cond::Subject(value.to_string()),
        "label" => Cond::Label(value.to_string()),
        "has" if value.eq_ignore_ascii_case("attachment") => Cond::HasAttachment,
        "after" => Cond::After(day(value)?),
        "before" => Cond::Before(day(value)?),
        "is" => Cond::Is(is(value)?),
        "in" if !negated => return Some(Op::Within(within(value)?)),
        _ => return None,
    };
    Some(Op::Cond(negated, cond))
}

fn day(value: &str) -> Option<i64> {
    let normal = value.replace('/', "-");
    let date = NaiveDate::parse_from_str(&normal, "%Y-%m-%d").ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

fn is(value: &str) -> Option<Is> {
    match value.to_ascii_lowercase().as_str() {
        "unread" => Some(Is::Unread),
        "read" => Some(Is::Read),
        "starred" => Some(Is::Starred),
        "pinned" => Some(Is::Pinned),
        _ => None,
    }
}

fn within(value: &str) -> Option<Within> {
    match value.to_ascii_lowercase().as_str() {
        "inbox" => Some(Within::Inbox),
        "archive" | "archived" => Some(Within::Archive),
        "junk" | "spam" => Some(Within::Junk),
        "trash" => Some(Within::Trash),
        "sent" => Some(Within::Sent),
        "starred" => Some(Within::Starred),
        "anywhere" => Some(Within::Anywhere),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread<'a>(labels: &'a [String]) -> ThreadFacts<'a> {
        ThreadFacts {
            subject: "Invoice for October",
            senders: "Alice <alice@shop.example>",
            latest_date: 1_761_955_200, // 2025-11-01
            unread_count: 1,
            starred: false,
            pinned: true,
            labels,
        }
    }

    fn msg(sender: &str, to: &str, attachments: Option<u32>) -> MessageFacts {
        MessageFacts {
            sender: sender.into(),
            recipients: to.into(),
            subject: "Invoice for October".into(),
            attachments,
        }
    }

    #[test]
    fn operators_and_text_separate() {
        let q = Query::parse(r#"from:alice "monthly report" has:attachment invoice in:anywhere"#);
        assert_eq!(q.text, "monthly report invoice");
        assert_eq!(
            q.conds,
            vec![
                (false, Cond::From("alice".into())),
                (false, Cond::HasAttachment)
            ]
        );
        assert_eq!(q.within, Some(Within::Anywhere));
    }

    #[test]
    fn quoted_values_and_negation() {
        let q = Query::parse(r#"subject:"q3 plan" -from:bob"#);
        assert_eq!(
            q.conds,
            vec![
                (false, Cond::Subject("q3 plan".into())),
                (true, Cond::From("bob".into()))
            ]
        );
        assert!(q.text.is_empty());
    }

    #[test]
    fn what_is_not_an_operator_is_text() {
        for input in [
            "foo:bar",
            "after:2026-13-01",
            "is:weird",
            "from:",
            "in:nowhere",
        ] {
            let q = Query::parse(input);
            assert!(q.conds.is_empty(), "{input}");
            assert_eq!(q.within, None, "{input}");
            assert_eq!(q.text, input, "{input}");
        }
        // a quoted token is a phrase even when it looks like an operator
        assert_eq!(Query::parse(r#""from:x""#).text, "from:x");
    }

    #[test]
    fn in_alone_is_an_operator() {
        assert!(Query::parse("in:junk").has_operators());
        assert!(!Query::parse("hello").has_operators());
    }

    #[test]
    fn dates_are_utc_midnight_in_both_spellings() {
        let a = Query::parse("after:2025-11-01");
        let b = Query::parse("after:2025/11/01");
        assert_eq!(a.conds, vec![(false, Cond::After(1_761_955_200))]);
        assert_eq!(a, b);
    }

    #[test]
    fn each_operator_admits_and_refuses() {
        let labels = vec!["Work".to_string()];
        let t = thread(&labels);
        let msgs = || {
            vec![msg(
                "Alice <alice@shop.example>",
                "me@golia.jp, Carol <carol@x.example>",
                Some(1),
            )]
        };
        let yes = [
            "from:ALICE",
            "to:carol",
            "subject:october",
            "has:attachment",
            "after:2025-11-01",
            "before:2025-11-02",
            "label:work",
            "is:unread",
            "is:pinned",
            "-is:starred",
            "-from:bob",
        ];
        for q in yes {
            assert!(Query::parse(q).admits(&t, msgs), "{q} should admit");
        }
        let no = [
            "from:bob",
            "to:dave",
            "subject:september",
            "-has:attachment",
            "after:2025-11-02",
            "before:2025-11-01",
            "label:home",
            "is:read",
            "is:starred",
            "-from:alice",
        ];
        for q in no {
            assert!(!Query::parse(q).admits(&t, msgs), "{q} should refuse");
        }
    }

    #[test]
    fn in_names_one_place_each() {
        let inbox = Place::default();
        let archived = Place {
            archived: true,
            ..inbox
        };
        let junk = Place {
            junk: true,
            ..inbox
        };
        let trashed = Place {
            trashed: true,
            archived: true,
            ..inbox
        };
        let sent_only = Place {
            has_sent: true,
            sent_only: true,
            ..inbox
        };
        assert!(Within::Inbox.admits(inbox));
        assert!(!Within::Inbox.admits(archived));
        assert!(!Within::Inbox.admits(junk));
        assert!(!Within::Inbox.admits(trashed));
        assert!(!Within::Inbox.admits(sent_only));
        assert!(Within::Archive.admits(archived));
        assert!(!Within::Archive.admits(trashed));
        assert!(Within::Junk.admits(junk));
        assert!(Within::Trash.admits(trashed));
        assert!(!Within::Trash.admits(inbox));
        assert!(Within::Sent.admits(sent_only));
        for p in [inbox, archived, junk, trashed, sent_only] {
            assert!(Within::Anywhere.admits(p));
        }
    }

    #[test]
    fn mail_never_counted_has_no_attachment() {
        let t = thread(&[]);
        let q = Query::parse("has:attachment");
        assert!(!q.admits(&t, || vec![msg("a", "b", None)]));
        assert!(!q.admits(&t, || vec![msg("a", "b", Some(0))]));
    }

    #[test]
    fn the_row_decides_without_reading_mail_when_it_can() {
        let t = thread(&[]);
        let q = Query::parse("from:alice is:unread after:2025-01-01");
        assert!(q.admits(&t, || panic!("messages read for what the row answers")));
    }

    #[test]
    fn messages_are_read_once_for_many_operators() {
        let t = thread(&[]);
        let calls = std::cell::Cell::new(0);
        let q = Query::parse("to:carol has:attachment from:zed");
        let admitted = q.admits(&t, || {
            calls.set(calls.get() + 1);
            vec![msg("zed@x", "carol@x", Some(2))]
        });
        assert!(admitted);
        assert_eq!(calls.get(), 1);
    }
}

//! What a delivered message says about itself, read once for every core:
//! its id, the time it sorts by, the conversation it joins, the preview
//! line, whether the recipient wrote it, and the folder category it is
//! filed under.

use mailrs_inbound::reason::{self, Reason};

use crate::headers::{
    body_text_for_search, extract_headers, extract_sender_trust, message_id_or_file_id, stored_date,
};

/// A core's per-user thread index, as threading asks about it. `None`
/// means nothing is known, whether or not the store could be read: a
/// lookup that fails starts a conversation rather than failing delivery.
pub trait Threads {
    /// The thread a message with this id landed in for `user`.
    fn thread_for_message_id(&self, user: &str, message_id: &str) -> Option<String>;
    /// The subject the conversation's row shows: its newest inbound
    /// message's, or the first message's when the user wrote them all.
    fn thread_subject(&self, user: &str, thread_id: &str) -> Option<String>;
}

/// Everything ingest derives from the message bytes.
pub struct Arrival<'a> {
    /// The `Message-ID`, or one made from the file name when it has none.
    pub message_id: String,
    pub in_reply_to: String,
    pub references: Vec<String>,
    pub subject: String,
    /// The `From:` header, decoded.
    pub from: String,
    /// The `To:` header, decoded.
    pub to: String,
    /// Seconds: the `Date:` header, unless missing or after arrival.
    pub date: i64,
    pub thread_id: String,
    /// The recipient wrote it: it is filed as sent, and read.
    pub is_own: bool,
    pub category: &'static str,
    /// Why it was filed where it was, beyond what the receiver's verdict
    /// says: the rule or classifier that chose `category`, or the
    /// recipient's filter that put it in Junk. Stored with the verdict.
    pub placement: Vec<Reason>,
    /// The line under the subject in the list.
    pub preview: String,
    /// The searchable text: the plain part, else the html part flattened.
    pub body_text: Option<String>,
    pub sender_trust: String,
    /// What the sectioned inbox reads from this message.
    pub facts: mailrs_mailbox::section::MessageFacts,
    /// The first 16 KB, where the headers are.
    pub head: &'a [u8],
    pub parsed: mailrs_mime::Part<'a>,
}

/// Read `body`, delivered to `addr` as `blob_ref` (its maildir file name,
/// with the subfolder when filed), into everything ingest needs.
/// `target_folder` is where the receiver's verdict or sieve filed it.
pub fn derive<'a, T: Threads + ?Sized>(
    threads: &T,
    mut net: Option<kevy_client::Connection>,
    addr: &str,
    blob_ref: &str,
    body: &'a [u8],
    target_folder: &str,
    // this server's authserv-id, for the sender-trust badge
    authserv: Option<&str>,
) -> Arrival<'a> {
    let head = &body[..body.len().min(16 * 1024)];
    let (header_id, in_reply_to, references, subject, date, from, to) = extract_headers(head);
    // Without a Message-ID the file name supplies one, so every path that
    // reads the file arrives at the same id.
    let message_id = message_id_or_file_id(header_id, blob_ref);
    let date = stored_date(date, blob_ref);
    let thread_id = thread_root(
        threads,
        addr,
        &message_id,
        &in_reply_to,
        &references,
        &subject,
    );
    let is_own = mailrs_rfc5322::list_contains(&from, addr);
    let rule = sender_rule_category(net.as_mut(), addr, &from, target_folder);
    let category_of = match rule {
        Some(c) => c,
        None => category(net.as_mut(), target_folder, body),
    };
    // the user's own mail is filed as sent; nobody judged where it goes
    let placement = match is_own {
        true => Vec::new(),
        false => placement(rule.is_some(), category_of, target_folder),
    };
    let facts =
        crate::section_facts::facts(net.as_mut(), addr, &from, &to, head, category_of, is_own);
    let category = category_of;
    let parsed = mailrs_mime::parse(body);
    let body_text = body_text_for_search(&parsed);
    let preview = body_text
        .as_deref()
        .map(|t| mailrs_clean::preview_line(t, 120))
        .unwrap_or_default();
    Arrival {
        message_id,
        in_reply_to,
        references,
        subject,
        from,
        to,
        date,
        thread_id,
        is_own,
        category,
        placement,
        preview,
        body_text,
        sender_trust: extract_sender_trust(body, authserv),
        facts,
        head,
        parsed,
    }
}

/// The conversation this message belongs to: the thread an ancestor
/// actually landed in when the subjects agree, a conversation of its own
/// when they do not, and with no ancestor known, the oldest reference,
/// else what it replies to, else itself. `References[0]` alone is not a
/// stable root (each hop can rewrite it), which is how conversations
/// fragmented.
pub fn thread_root<T: Threads + ?Sized>(
    threads: &T,
    addr: &str,
    message_id: &str,
    in_reply_to: &str,
    references: &[String],
    subject: &str,
) -> String {
    match ancestry(threads, addr, message_id, in_reply_to, references, subject) {
        Ancestry::Joins(tid) => tid,
        // The fallback below would name the ancestor's thread again: its id
        // is usually the root message's, which is what In-Reply-To and
        // References[0] cite.
        Ancestry::NewTopic => message_id.to_string(),
        Ancestry::Unknown => {
            if let Some(first) = references.first() {
                first.clone()
            } else if !in_reply_to.is_empty() {
                in_reply_to.to_string()
            } else {
                message_id.to_string()
            }
        }
    }
}

enum Ancestry {
    /// the thread the message, or its nearest ancestor, is in
    Joins(String),
    /// an ancestor is known, under another subject
    NewTopic,
    Unknown,
}

/// The message's own id first — a message already ingested (and perhaps
/// moved by a rethread merge) lands back where it is — then the nearest
/// ancestor: In-Reply-To, then References newest to oldest.
fn ancestry<T: Threads + ?Sized>(
    threads: &T,
    user: &str,
    own_mid: &str,
    in_reply_to: &str,
    references: &[String],
    subject: &str,
) -> Ancestry {
    if !own_mid.is_empty()
        && let Some(tid) = threads.thread_for_message_id(user, own_mid)
    {
        // no subject gate: the message is already in that thread, and
        // splitting it here would fight the recorded state
        return Ancestry::Joins(tid);
    }
    let mut candidate: Option<String> = None;
    if !in_reply_to.is_empty() {
        candidate = threads.thread_for_message_id(user, in_reply_to);
    }
    if candidate.is_none() {
        candidate = references
            .iter()
            .rev()
            .find_map(|mid| threads.thread_for_message_id(user, mid));
    }
    // An ancestor's conversation is joined only when the normalized
    // subjects agree: a reply that changes topic starts a new one, or the
    // old thread's row would read as the user's own outbound subject.
    let Some(tid) = candidate else {
        return Ancestry::Unknown;
    };
    let subj_norm = mailrs_mailbox::threading::normalize_subject(subject);
    if subj_norm.is_empty() {
        return Ancestry::Joins(tid);
    }
    match threads.thread_subject(user, &tid) {
        Some(row) if mailrs_mailbox::threading::normalize_subject(&row) != subj_norm => {
            Ancestry::NewTopic
        }
        _ => Ancestry::Joins(tid),
    }
}

/// Who filed the arrival out of the plain inbox: the reader's rule for the
/// sender, the classifier, or the reader's filter putting it in Junk.
/// Nothing for mail left in the plain inbox, whoever left it there: a
/// record is stored per recipient for each of these, and the inbox is
/// where most mail goes.
pub fn placement(by_rule: bool, category: &str, target_folder: &str) -> Vec<Reason> {
    let mut out = Vec::new();
    if category != "inbox" && category != "spam" {
        let code = match by_rule {
            true => reason::SORTED_BY_YOUR_RULE,
            false => reason::SORTED_BY_CLASSIFIER,
        };
        out.push(Reason::new(code).with("class", category));
    }
    if target_folder.eq_ignore_ascii_case("junk") {
        // the receiver's own Junk reason, when it gave one, outranks this:
        // see `why::stored`
        out.push(Reason::new(reason::JUNK_YOUR_FILTER));
    }
    out
}

/// The category the reader's own rule for this sender files it under,
/// ahead of the classifier; never for mail the receiver or Sieve put in
/// Junk. A rule is the reader's word, so it is not evidence to train on.
pub fn sender_rule_category(
    net: Option<&mut kevy_client::Connection>,
    addr: &str,
    from: &str,
    target_folder: &str,
) -> Option<&'static str> {
    if target_folder.eq_ignore_ascii_case("junk") {
        return None;
    }
    let rule = mailrs_core_sidestate::families::sender_rules::get(net?, addr, from).ok()??;
    match rule.bucket.as_deref()? {
        "inbox" => Some("inbox"),
        "notifications" => Some("notification"),
        "promotions" => Some("promotion"),
        _ => None,
    }
}

/// The category the arrival is filed under. The receiver's Junk verdict
/// (or a sieve fileinto Junk) is spam; anything else is sorted into inbox,
/// notification or promotion by the shared triage corpus, inbox when it is
/// cold or unsure.
pub fn category(
    net: Option<&mut kevy_client::Connection>,
    target_folder: &str,
    body: &[u8],
) -> &'static str {
    if target_folder.eq_ignore_ascii_case("junk") {
        return "spam";
    }
    net.and_then(|conn| mailrs_triage::classify(conn, body))
        .unwrap_or("inbox")
}

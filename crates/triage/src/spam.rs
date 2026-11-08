//! The spam/ham corpus the receiver scores inbound mail against, and the
//! training a reader's "junk" / "not junk" feeds it.
//!
//! Storage keys (network kevy), for each [`Corpus`]:
//!
//! ```text
//! bayes:v2:tokens:spam, bayes:v2:tokens:ham   hash  field=token value=thread count
//! bayes:v2:meta                               hash  {spam_msgs, ham_msgs}
//! bayes:v2:corpus:spam, bayes:v2:corpus:ham   set   thread ids, kept for a retrain
//! bayes:v2:trained:<tid>                      string  spam|ham  (TTL 90d)
//! ```
//!
//! and the same without `v2:` for the legacy corpus. Two corpora exist
//! because the tokenizer changed: counts made by one tokenizer do not match
//! the tokens of the other. Training writes both; the receiver reads
//! [`READ`], with that corpus's tokenizer.
//!
//! A thread is one training sample, its tokens the union across its
//! messages. Training is reversible: a thread trained the other way is
//! unlearned first, so correcting a mis-file does not count it twice.

use std::collections::BTreeSet;

use kevy_client::Connection;

/// A spam/ham corpus and the tokenizer whose tokens it counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corpus {
    /// Counts of [`mailrs_bayes::tokenize_raw`], the raw-byte tokenizer.
    Legacy,
    /// Counts of [`mailrs_bayes::tokenize`], which reads decoded text.
    V2,
}

impl Corpus {
    /// Every corpus; training writes each of them.
    pub const ALL: [Corpus; 2] = [Corpus::Legacy, Corpus::V2];

    /// The tokenizer whose tokens this corpus counts.
    pub fn tokenizer(self) -> mailrs_parse_worker::Tokenizer {
        match self {
            Corpus::Legacy => mailrs_parse_worker::Tokenizer::Raw,
            Corpus::V2 => mailrs_parse_worker::Tokenizer::Decoded,
        }
    }

    /// The tokens this corpus counts, for one raw message, read in the
    /// parse worker; none when it could not read them.
    pub fn tokenize(self, raw: &[u8]) -> Vec<String> {
        mailrs_parse_worker::tokens(raw, self.tokenizer(), mailrs_parse_worker::Limit::Batch)
            .read()
            .unwrap_or_default()
    }
}

/// The corpus the receiver's spam score reads: v2, once
/// `maintenance:bayes-rebuild-v2` has retrained it from what readers and
/// the fraud hold judged spam (the legacy corpus learnt its spam from the
/// server's own score).
pub const READ: Corpus = Corpus::V2;

/// The keys of one spam/ham corpus.
pub struct Keys {
    /// Per-token spam counts.
    pub spam_tokens: &'static [u8],
    /// Per-token ham counts.
    pub ham_tokens: &'static [u8],
    /// Corpus totals, `spam_msgs` and `ham_msgs`.
    pub meta: &'static [u8],
    /// The thread ids trained as spam.
    pub corpus_spam: &'static [u8],
    /// The thread ids trained as ham.
    pub corpus_ham: &'static [u8],
}

const LEGACY: Keys = Keys {
    spam_tokens: b"bayes:tokens:spam",
    ham_tokens: b"bayes:tokens:ham",
    meta: b"bayes:meta",
    corpus_spam: b"bayes:corpus:spam",
    corpus_ham: b"bayes:corpus:ham",
};

const V2: Keys = Keys {
    spam_tokens: b"bayes:v2:tokens:spam",
    ham_tokens: b"bayes:v2:tokens:ham",
    meta: b"bayes:v2:meta",
    corpus_spam: b"bayes:v2:corpus:spam",
    corpus_ham: b"bayes:v2:corpus:ham",
};

const TRAINED_TTL_SECS: u64 = 90 * 24 * 3600;

/// The keys of `corpus`.
pub fn keys(corpus: Corpus) -> &'static Keys {
    match corpus {
        Corpus::Legacy => &LEGACY,
        Corpus::V2 => &V2,
    }
}

fn trained_key(corpus: Corpus, thread_id: &str) -> String {
    match corpus {
        Corpus::Legacy => format!("bayes:trained:{thread_id}"),
        Corpus::V2 => format!("bayes:v2:trained:{thread_id}"),
    }
}

/// The direction `corpus` last trained `thread_id`, while its 90-day
/// marker lasts: `Some(true)` spam, `Some(false)` ham.
pub fn trained_as_spam(conn: &mut Connection, corpus: Corpus, thread_id: &str) -> Option<bool> {
    let v = conn
        .get(trained_key(corpus, thread_id).as_bytes())
        .ok()
        .flatten()?;
    match v.as_slice() {
        b"spam" => Some(true),
        b"ham" => Some(false),
        _ => None,
    }
}

/// Learn the thread whose messages are `raws` as spam (`is_spam`) or ham,
/// in every corpus.
///
/// Best effort: a training write that fails is dropped, because training
/// must never fail the reader's action it follows.
pub fn train(conn: &mut Connection, thread_id: &str, raws: &[Vec<u8>], is_spam: bool) {
    for corpus in Corpus::ALL {
        train_in(conn, corpus, thread_id, raws, is_spam);
    }
}

/// [`train`], in one corpus only.
pub fn train_in(
    conn: &mut Connection,
    corpus: Corpus,
    thread_id: &str,
    raws: &[Vec<u8>],
    is_spam: bool,
) {
    let tokens: BTreeSet<String> = raws.iter().flat_map(|r| corpus.tokenize(r)).collect();
    if tokens.is_empty() {
        return;
    }
    let k = keys(corpus);
    let key = trained_key(corpus, thread_id);
    let prior = conn
        .get(key.as_bytes())
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok());
    let now = if is_spam { "spam" } else { "ham" };
    let tokens: Vec<&str> = tokens.iter().map(String::as_str).collect();
    if prior.as_deref() != Some(now) {
        if let Some(prev) = prior.as_deref() {
            let prev_spam = prev == "spam";
            adjust(conn, k, &tokens, prev_spam, -1);
            let _ = conn.srem(set_of(k, prev_spam), &[thread_id.as_bytes()]);
        }
        adjust(conn, k, &tokens, is_spam, 1);
        let _ = conn.sadd(set_of(k, is_spam), &[thread_id.as_bytes()]);
        tracing::info!(%thread_id, is_spam, ?corpus, tokens = tokens.len(), "bayes: trained thread");
    }
    let _ = conn.set(key.as_bytes(), now.as_bytes());
    let _ = conn.expire(
        key.as_bytes(),
        std::time::Duration::from_secs(TRAINED_TTL_SECS),
    );
}

/// Empty `corpus`: its counts, its totals, its sets and the marker of every
/// thread in them, so a retrain starts from nothing and is not skipped as
/// already done. Returns how many threads it held.
///
/// # Errors
/// The store could not be read or written.
pub fn clear(conn: &mut Connection, corpus: Corpus) -> std::io::Result<usize> {
    let k = keys(corpus);
    let mut threads: BTreeSet<Vec<u8>> = conn.smembers(k.corpus_spam)?.into_iter().collect();
    threads.extend(conn.smembers(k.corpus_ham)?);
    for tid in &threads {
        let tid = String::from_utf8_lossy(tid);
        conn.del(&[trained_key(corpus, &tid).as_bytes()])?;
    }
    conn.del(&[
        k.spam_tokens,
        k.ham_tokens,
        k.meta,
        k.corpus_spam,
        k.corpus_ham,
    ])?;
    Ok(threads.len())
}

fn set_of(k: &Keys, spam: bool) -> &'static [u8] {
    if spam { k.corpus_spam } else { k.corpus_ham }
}

/// Move every token's count, and the corpus total, by `delta`.
fn adjust(conn: &mut Connection, k: &Keys, tokens: &[&str], spam: bool, delta: i64) {
    let key = if spam { k.spam_tokens } else { k.ham_tokens };
    let field: &[u8] = if spam { b"spam_msgs" } else { b"ham_msgs" };
    let delta = delta.to_string();
    let _ = conn.pipeline(|p| {
        for t in tokens {
            p.cmd(&[b"HINCRBY", key, t.as_bytes(), delta.as_bytes()]);
        }
        p.cmd(&[b"HINCRBY", k.meta, field, delta.as_bytes()]);
    });
}

/// Learn the thread whose messages are `raws` as triage `class`.
pub fn train_class(conn: &mut Connection, thread_id: &str, raws: &[Vec<u8>], class: &str) {
    if !crate::is_class(class) {
        return;
    }
    let tokens: BTreeSet<String> = raws.iter().flat_map(|r| crate::tokenize(r)).collect();
    if tokens.is_empty() {
        return;
    }
    let tokens: Vec<String> = tokens.into_iter().collect();
    crate::train(conn, &tokens, class, thread_id);
}

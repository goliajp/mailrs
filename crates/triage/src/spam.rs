//! The spam/ham corpus the receiver scores inbound mail against, and the
//! training a reader's "junk" / "not junk" feeds it.
//!
//! Storage keys (network kevy):
//!
//! ```text
//! bayes:v2:tokens:spam, bayes:v2:tokens:ham   hash  field=token value=thread count
//! bayes:v2:meta                               hash  {spam_msgs, ham_msgs}
//! bayes:v2:corpus:spam, bayes:v2:corpus:ham   set   thread ids, kept for a retrain
//! bayes:v2:trained:<tid>                      string  spam|ham  (TTL 90d)
//! ```
//!
//! `v2` is the corpus of the tokenizer that reads decoded text. The
//! unversioned keys hold the corpus of the raw-byte tokenizer; nothing
//! reads its counts any more, and its corpus sets and markers are read
//! only by the rebuild that retrains v2 from them.
//!
//! A thread is one training sample, its tokens the union across its
//! messages. Training is reversible: a thread trained the other way is
//! unlearned first, so correcting a mis-file does not count it twice.

use std::collections::BTreeSet;

use kevy_client::Connection;

/// Per-token spam counts.
pub const SPAM_TOKENS: &[u8] = b"bayes:v2:tokens:spam";
/// Per-token ham counts.
pub const HAM_TOKENS: &[u8] = b"bayes:v2:tokens:ham";
/// Corpus totals, `spam_msgs` and `ham_msgs`.
pub const META: &[u8] = b"bayes:v2:meta";
const CORPUS_SPAM: &[u8] = b"bayes:v2:corpus:spam";
const CORPUS_HAM: &[u8] = b"bayes:v2:corpus:ham";
/// The thread ids the raw-byte corpus was trained on as spam.
pub const LEGACY_CORPUS_SPAM: &[u8] = b"bayes:corpus:spam";
/// The thread ids the raw-byte corpus was trained on as ham.
pub const LEGACY_CORPUS_HAM: &[u8] = b"bayes:corpus:ham";
const TRAINED_TTL_SECS: u64 = 90 * 24 * 3600;

fn trained_key(thread_id: &str) -> String {
    format!("bayes:v2:trained:{thread_id}")
}

/// The direction the raw-byte corpus last trained `thread_id`, while its
/// 90-day marker lasts: `Some(true)` spam, `Some(false)` ham.
pub fn legacy_trained_as_spam(conn: &mut Connection, thread_id: &str) -> Option<bool> {
    let key = format!("bayes:trained:{thread_id}");
    let v = conn.get(key.as_bytes()).ok().flatten()?;
    match v.as_slice() {
        b"spam" => Some(true),
        b"ham" => Some(false),
        _ => None,
    }
}

/// Learn the thread whose messages are `raws` as spam (`is_spam`) or ham.
///
/// Best effort: a training write that fails is dropped, because training
/// must never fail the reader's action it follows.
pub fn train(conn: &mut Connection, thread_id: &str, raws: &[Vec<u8>], is_spam: bool) {
    let tokens: BTreeSet<String> = raws
        .iter()
        .flat_map(|r| mailrs_bayes::tokenize(r))
        .collect();
    if tokens.is_empty() {
        return;
    }
    let key = trained_key(thread_id);
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
            adjust(conn, &tokens, prev_spam, -1);
            let _ = conn.srem(corpus(prev_spam), &[thread_id.as_bytes()]);
        }
        adjust(conn, &tokens, is_spam, 1);
        let _ = conn.sadd(corpus(is_spam), &[thread_id.as_bytes()]);
        tracing::info!(%thread_id, is_spam, tokens = tokens.len(), "bayes: trained thread");
    }
    let _ = conn.set(key.as_bytes(), now.as_bytes());
    let _ = conn.expire(
        key.as_bytes(),
        std::time::Duration::from_secs(TRAINED_TTL_SECS),
    );
}

fn corpus(spam: bool) -> &'static [u8] {
    if spam { CORPUS_SPAM } else { CORPUS_HAM }
}

/// Move every token's count, and the corpus total, by `delta`.
fn adjust(conn: &mut Connection, tokens: &[&str], spam: bool, delta: i64) {
    let key = if spam { SPAM_TOKENS } else { HAM_TOKENS };
    let field: &[u8] = if spam { b"spam_msgs" } else { b"ham_msgs" };
    let delta = delta.to_string();
    let _ = conn.pipeline(|p| {
        for t in tokens {
            p.cmd(&[b"HINCRBY", key, t.as_bytes(), delta.as_bytes()]);
        }
        p.cmd(&[b"HINCRBY", META, field, delta.as_bytes()]);
    });
}

/// Learn the thread whose messages are `raws` as triage `class`.
pub fn train_class(conn: &mut Connection, thread_id: &str, raws: &[Vec<u8>], class: &str) {
    if !crate::is_class(class) {
        return;
    }
    let tokens: BTreeSet<String> = raws
        .iter()
        .flat_map(|r| mailrs_bayes::tokenize(r))
        .collect();
    if tokens.is_empty() {
        return;
    }
    let tokens: Vec<String> = tokens.into_iter().collect();
    crate::train(conn, &tokens, class, thread_id);
}

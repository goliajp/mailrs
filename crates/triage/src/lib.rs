//! Multi-class email triage corpus, network-kevy backed.
//!
//! The pure classification math lives in `mailrs-bayes`
//! ([`classify_multiclass`]); this crate is the shared kevy glue both
//! serving lanes (fastcore + the monolith) call so they classify with
//! ONE corpus and agree on every verdict.
//!
//! Storage keys (network kevy), for each [`Corpus`]:
//!
//! ```text
//! bayes:triage:v2:tokens:<class>   hash  field=token value=message-count
//! bayes:triage:v2:meta             hash  {<class>_msgs}
//! bayes:triage:v2:trained:<tid>    string  <class>  (TTL 90d)
//! ```
//!
//! and the same without `v2:` for the legacy corpus. The last key holds the
//! class a thread was last trained as, so a re-file unlearns the prior class
//! before learning the new one.
//!
//! Two corpora exist because the tokenizer changed: counts made by one
//! tokenizer do not match the tokens of the other. Training writes both;
//! classification reads [`READ`], with that corpus's tokenizer.

use kevy_client::Connection;
use mailrs_bayes::{MultiCorpus, classify_multiclass};

pub mod spam;

/// The triage classes, in a stable order the corpus counts align with.
pub const CLASSES: [&str; 3] = ["inbox", "notification", "promotion"];

/// A corpus and the tokenizer whose tokens it counts.
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

    /// The tokens this corpus counts, for one raw message.
    pub fn tokenize(self, raw: &[u8]) -> Vec<String> {
        match self {
            Corpus::Legacy => mailrs_bayes::tokenize_raw(raw),
            Corpus::V2 => mailrs_bayes::tokenize(raw),
        }
    }
}

/// The corpus classification reads, spam/ham and triage alike.
pub const READ: Corpus = Corpus::Legacy;

const TRAINED_TTL_SECS: u64 = 90 * 24 * 3600;
/// One-vs-rest winning probability required before a verdict is
/// applied — below this the caller defaults to Inbox.
const MIN_CONFIDENCE: f64 = 0.75;

fn meta_key(corpus: Corpus) -> &'static [u8] {
    match corpus {
        Corpus::Legacy => b"bayes:triage:meta",
        Corpus::V2 => b"bayes:triage:v2:meta",
    }
}

fn tokens_key(corpus: Corpus, class: &str) -> String {
    match corpus {
        Corpus::Legacy => format!("bayes:triage:tokens:{class}"),
        Corpus::V2 => format!("bayes:triage:v2:tokens:{class}"),
    }
}

fn marker_key(corpus: Corpus, thread_id: &str) -> String {
    match corpus {
        Corpus::Legacy => format!("bayes:triage:trained:{thread_id}"),
        Corpus::V2 => format!("bayes:triage:v2:trained:{thread_id}"),
    }
}

/// True if `class` is one of the triage classes.
pub fn is_class(class: &str) -> bool {
    CLASSES.contains(&class)
}

/// Classify a raw message into a triage class name ("inbox" |
/// "notification" | "promotion") against the [`READ`] corpus. Returns
/// `None` on cold-start / low confidence — the caller then defaults to
/// "inbox". Reads the corpus from `conn` (network kevy).
pub fn classify(conn: &mut Connection, raw: &[u8]) -> Option<&'static str> {
    let tokens = READ.tokenize(raw);
    if tokens.is_empty() {
        return None;
    }
    let corpus = class_totals(conn, READ);

    // Per-token per-class counts: for each token, HGET all class hashes
    // in one pipeline.
    let tkeys: Vec<String> = CLASSES.iter().map(|c| tokens_key(READ, c)).collect();
    let n_classes = CLASSES.len();
    let replies = conn
        .pipeline(|p| {
            for t in &tokens {
                for tk in &tkeys {
                    p.cmd(&[b"HGET", tk.as_bytes(), t.as_bytes()]);
                }
            }
        })
        .ok()?;
    let mut counts: std::collections::HashMap<String, Vec<u32>> =
        std::collections::HashMap::with_capacity(tokens.len());
    for (ti, tok) in tokens.iter().enumerate() {
        let mut per_class = vec![0u32; n_classes];
        for (ci, slot) in per_class.iter_mut().enumerate() {
            if let Some(kevy_client::Reply::Bulk(b)) = replies.get(ti * n_classes + ci) {
                *slot = std::str::from_utf8(b)
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0)
                    .max(0) as u32;
            }
        }
        if per_class.iter().any(|v| *v > 0) {
            counts.insert(tok.clone(), per_class);
        }
    }

    let idx = classify_multiclass(&tokens, &corpus, |t| counts.get(t).cloned(), MIN_CONFIDENCE)?;
    CLASSES.get(idx).copied()
}

/// How many threads `corpus` has learnt per class, in [`CLASSES`] order.
pub fn class_totals(conn: &mut Connection, corpus: Corpus) -> MultiCorpus {
    MultiCorpus {
        classes: CLASSES
            .iter()
            .map(|c| {
                let field = format!("{c}_msgs");
                let n = conn
                    .hget(meta_key(corpus), field.as_bytes())
                    .ok()
                    .flatten()
                    .and_then(|v| String::from_utf8(v).ok())
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0)
                    .max(0) as u32;
                ((*c).to_string(), n)
            })
            .collect(),
    }
}

/// Train `corpus` on `tokens` (the deduped union of a thread's messages,
/// from that corpus's tokenizer) as belonging to `class`. Reversible: if
/// `thread_id` was previously trained as a different class, that class is
/// unlearned first. Best-effort — a kevy hiccup is swallowed.
pub(crate) fn train(
    conn: &mut Connection,
    corpus: Corpus,
    tokens: &[String],
    class: &str,
    thread_id: &str,
) {
    if !is_class(class) || tokens.is_empty() {
        return;
    }
    let marker = marker_key(corpus, thread_id);
    let prior = conn
        .get(marker.as_bytes())
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok());
    if prior.as_deref() == Some(class) {
        refresh_marker(conn, &marker, class);
        return;
    }
    let token_list: Vec<&str> = tokens.iter().map(String::as_str).collect();
    if let Some(prev) = prior.as_deref() {
        adjust(conn, corpus, prev, &token_list, -1);
    }
    adjust(conn, corpus, class, &token_list, 1);
    refresh_marker(conn, &marker, class);
    tracing::info!(%thread_id, %class, tokens = token_list.len(), "triage: trained");
}

fn refresh_marker(conn: &mut Connection, marker: &str, class: &str) {
    let _ = conn.set(marker.as_bytes(), class.as_bytes());
    let _ = conn.expire(
        marker.as_bytes(),
        std::time::Duration::from_secs(TRAINED_TTL_SECS),
    );
}

/// HINCRBY the per-class token hash + the class message counter.
fn adjust(conn: &mut Connection, corpus: Corpus, class: &str, tokens: &[&str], delta: i64) {
    let tkey = tokens_key(corpus, class);
    let meta = meta_key(corpus);
    let field = format!("{class}_msgs");
    let delta_s = delta.to_string();
    let _ = conn.pipeline(|p| {
        for t in tokens {
            p.cmd(&[
                b"HINCRBY",
                tkey.as_bytes(),
                t.as_bytes(),
                delta_s.as_bytes(),
            ]);
        }
        p.cmd(&[b"HINCRBY", meta, field.as_bytes(), delta_s.as_bytes()]);
    });
}

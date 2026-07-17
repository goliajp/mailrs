//! `mailrs-bayes` — naive-Bayes spam classifier core.
//!
//! A pure, I/O-free stone: [`tokenize`] turns raw RFC 5322 message
//! bytes into a deduplicated feature-token set, and [`classify`] scores
//! a token set against caller-supplied training counts using the
//! Graham-Robinson method with Fisher chi-square combining.
//!
//! Storage, training persistence, and the SMTP-time decision are the
//! caller's job — this crate never touches the network or disk, so it
//! drops into any pipeline that can supply `(spam, ham)` token counts.
//!
//! ```
//! use mailrs_bayes::{classify, tokenize, Corpus, TokenCounts};
//!
//! let tokens = tokenize(b"Subject: cheap pills\r\n\r\nbuy now");
//! // With an untrained corpus the classifier stays silent.
//! let verdict = classify(&tokens, |_| None, &Corpus::default());
//! assert_eq!(verdict, None);
//! ```

#![forbid(unsafe_code)]

mod classify;
mod tokenize;

pub use classify::{Corpus, MultiCorpus, TokenCounts, classify, classify_multiclass};
pub use tokenize::tokenize;

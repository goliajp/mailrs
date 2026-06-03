#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Email intelligence primitives — LLM-powered analysis with a pluggable provider.
//!
//! `mailrs-intelligence` extracts the five analysis modules from the
//! `mailrs` mail server:
//!
//! - [`analyze`] — full email analysis (category, summary, entities, intent)
//! - [`spam`]    — spam classification with an optional cache
//! - [`importance`] — heuristic importance scoring (no LLM)
//! - [`structured`] — JSON-LD / Microdata extraction from HTML (no LLM)
//! - [`provider`] — the [`LlmProvider`] trait + an OpenAI-compatible reference impl
//!
//! # Why a trait
//!
//! Mail servers tend to mix cheap small-core inference (for hot paths like
//! per-message classification) with occasional big-core calls (for rare,
//! high-value structured extraction). Letting analysis functions take
//! `&dyn LlmProvider` keeps that choice **visible at the call site**: every
//! consumer can grep their own code for "which provider do I pass into
//! `analyze_email`?" without diving into config.
//!
//! # Quickstart
//!
//! ```no_run
//! use mailrs_intelligence::{OpenAiCompatibleProvider, analyze::analyze_email};
//!
//! # async fn run() -> Option<()> {
//! let provider = OpenAiCompatibleProvider::new(
//!     "http://llm.example.com/complete".into(),
//!     Some("sk-…".into()),
//!     "qwen3.5-9b/v8".into(),
//! );
//!
//! let analysis = analyze_email(
//!     &provider,
//!     "boss@example.com",
//!     "Q3 review",
//!     "Please send your Q3 self-review by Friday.",
//! )
//! .await?;
//!
//! println!("category={} requires_action={}", analysis.category, analysis.requires_action);
//! # Some(())
//! # }
//! ```
//!
//! # Feature flags
//!
//! - `http` (default) — enables [`OpenAiCompatibleProvider`], the reqwest-backed
//!   reference [`LlmProvider`].
//! - `kevy-cache` (default) — enables [`spam::KevySpamCache`], a Kevy-backed
//!   [`spam::SpamCache`] implementation. Disable if you cache yourself or run
//!   without a cache.
//!
//! Disable both default features if you're plugging in your own backends.

pub mod analyze;
pub mod importance;
/// Pluggable LLM provider trait (currently Claude / Ollama implementations).
pub mod provider;
pub mod spam;
/// Schema.org JSON-LD extraction from HTML message bodies.
pub mod structured;

#[cfg(feature = "http")]
mod openai_compatible;

pub use provider::LlmProvider;

#[cfg(feature = "http")]
pub use openai_compatible::OpenAiCompatibleProvider;

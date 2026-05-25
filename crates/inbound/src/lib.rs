#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![doc = include_str!("../README.md")]

//! Composable SMTP receive pipeline framework for Rust mail servers.
//!
//! ## What's in the box
//!
//! - [`Stage`] trait — one async check that reads + mutates the receive
//!   context and returns whether to continue or short-circuit with a
//!   decision.
//! - [`Pipeline`] + [`PipelineBuilder`] — sequential executor with
//!   early-reject.
//! - [`ReceiveContext`] — accumulator for static request data + the
//!   signals each stage contributes (auth results, virus, content score,
//!   PTR score, AI score).
//! - [`DeliveryDecision`] — the four-variant outcome
//!   (`Accept` / `Junk` / `Reject` / `Greylist`).
//! - [`make_delivery_decision`] — pure final-decision policy combiner.
//!   Callable directly if you don't want the [`Pipeline`] framework.
//! - [`build_auth_header`] / [`AuthResult`] / [`format_auth_results`] —
//!   RFC 8601 `Authentication-Results:` header builders.
//!
//! ## What's NOT in the box
//!
//! By design, the crate ships zero protocol / backend code:
//!
//! - No SPF / DKIM / DMARC verifier — wrap your favorite crate
//!   (`mail-auth`, `dkim-rs`, ...) inside your own [`Stage`] impl.
//! - No virus scanner — ClamAV, rspamd, whatever your stack uses.
//! - No greylisting backend — Redis / Memcached / Postgres / in-memory.
//! - No DNS resolver type.
//! - No LLM / ML scoring provider.
//!
//! That means **the crate is small, dependency-light, and trivial to drop
//! into any existing SMTP server**. The price is that every consumer
//! writes their own [`Stage`] impls — typically 50-150 LOC per stage,
//! see the README for example shapes.
//!
//! ## Sketch
//!
//! ```ignore
//! use mailrs_inbound::{Pipeline, ReceiveContext, Stage, StageOutcome};
//! use async_trait::async_trait;
//!
//! struct MyGreylistStage { /* ... */ }
//!
//! #[async_trait]
//! impl Stage for MyGreylistStage {
//!     fn name(&self) -> &str { "greylist" }
//!     async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
//!         // ... your greylist logic ...
//!         StageOutcome::Continue
//!     }
//! }
//!
//! # async fn build_and_run(stage: MyGreylistStage, mut ctx: ReceiveContext) {
//! let pipeline = Pipeline::builder()
//!     .add(stage)
//!     // ... more stages ...
//!     .build();
//!
//! let decision = pipeline.run(&mut ctx).await;
//! # let _ = decision;
//! # }
//! ```

pub mod auth_header;
pub mod context;
pub mod decision;
pub mod pipeline;
pub mod stage;

// Public re-exports — the surface most consumers reach for.
pub use auth_header::{
    AuthResult, build_auth_header, format_auth_results, format_auth_results_header,
};
pub use context::{AuthResults, DmarcPolicy, ReceiveContext};
pub use decision::{DeliveryDecision, PipelineInput, make_delivery_decision};
pub use pipeline::{DEFAULT_SPAM_THRESHOLD, Pipeline, PipelineBuilder};
pub use stage::{Stage, StageOutcome};

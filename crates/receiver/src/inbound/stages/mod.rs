//! `mailrs_inbound::Stage` impls — the 6 checks mailrs's inbound pipeline runs.
//!
//! Each stage wraps one backend (greylist store, DNS resolver, SPF/DKIM/DMARC
//! authenticator, rule engine, LLM provider) and translates its
//! result into context mutations + optional short-circuit decisions.
//!
//! Wired into a [`mailrs_inbound::Pipeline`] via
//! [`build_inbound_pipeline`](crate::inbound::pipeline::build_inbound_pipeline)
//! at server startup and reused across all inbound SMTP transactions.

pub mod ai_scoring;
pub mod bayes;
pub mod content_scan;
pub mod greylist;
pub mod local_lists;
pub mod mail_auth;
pub mod ptr;
pub mod quota;

pub use ai_scoring::AiScoringStage;
pub use bayes::BayesStage;
pub use content_scan::ContentScanStage;
pub use greylist::GreylistStage;
pub use local_lists::LocalListsStage;
pub use mail_auth::MailAuthStage;
pub use ptr::PtrStage;

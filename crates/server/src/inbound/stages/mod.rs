//! `mailrs_inbound::Stage` impls — the 6 checks mailrs's inbound pipeline runs.
//!
//! Each stage wraps one backend (greylist store, DNS resolver, SPF/DKIM/DMARC
//! authenticator, ClamAV socket, rule engine, LLM provider) and translates its
//! result into context mutations + optional short-circuit decisions.
//!
//! Wired into a [`mailrs_inbound::Pipeline`] via
//! [`build_inbound_pipeline`](crate::inbound::pipeline::build_inbound_pipeline)
//! at server startup and reused across all inbound SMTP transactions.

pub mod ai_scoring;
pub mod clamav;
pub mod content_scan;
pub mod greylist;
pub mod mail_auth;
pub mod ptr;

pub use ai_scoring::AiScoringStage;
pub use clamav::ClamavStage;
pub use content_scan::ContentScanStage;
pub use greylist::GreylistStage;
pub use mail_auth::MailAuthStage;
pub use ptr::PtrStage;

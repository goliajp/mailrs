//! Inbound aggregate-report ingestion — the receiving half of RFC 7489
//! §7.2.
//!
//! The rest of this crate covers evaluating inbound mail against a
//! DMARC policy and *generating* aggregate reports for domains we
//! receive mail on behalf of. This module covers the third face:
//! reading the reports other receivers send **us**, so a domain owner
//! can see who is sending as their domain and whether it aligns.
//!
//! Two pure functions, no I/O:
//!
//! ```no_run
//! use mailrs_dmarc::ingest::{extract_report_payloads, parse_aggregate_report};
//!
//! # let raw_message: &[u8] = b"";
//! for payload in extract_report_payloads(raw_message) {
//!     match parse_aggregate_report(&payload) {
//!         Ok(report) => {
//!             println!(
//!                 "{} reported {} messages, {} passing",
//!                 report.report_metadata.org_name,
//!                 report.total_count(),
//!                 report.passing_count(),
//!             );
//!         }
//!         Err(e) => eprintln!("unparseable report: {e}"),
//!     }
//! }
//! ```
//!
//! Both are trust-boundary code: the input is mail from anyone who
//! cares to send it. Every failure path returns rather than panics, and
//! both payload size and row count are bounded.

mod envelope;
mod model;
mod xml;

pub use envelope::extract_report_payloads;
pub use model::{
    AggregateReport, AuthResults, DateRange, DkimAuthResult, Identifiers, PolicyEvaluated,
    PolicyPublished, ReportMetadata, ReportRecord, Row, SpfAuthResult,
};
pub use xml::{IngestError, MAX_PAYLOAD_BYTES, MAX_RECORDS, parse_aggregate_report};

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![doc = include_str!("../README.md")]

pub mod error;
pub mod failure;
pub mod record;
pub mod report;

pub use error::TlsRptError;
pub use failure::FailureType;
pub use record::{RuaEndpoint, TlsRptRecord};
pub use report::{
    DateRange, FailureDetail, FailureEvent, PolicyBlock, PolicyReport, PolicyType, Report,
    ReportBuilder, SuccessEvent, SummaryBlock,
};

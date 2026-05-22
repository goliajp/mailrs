#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Module layout:
//! - [`record`] — TXT-record string → typed [`Record`]
//! - [`evaluator`] — typed Record + DNS → [`SpfResult`]
//! - [`resolver`] — [`SpfResolver`] trait + (optional) hickory impl
//! - [`error`] — error / temp-fail / perm-fail types

pub mod error;
pub mod evaluator;
pub mod record;
pub mod resolver;

pub use error::{SpfError, SpfResult};
pub use evaluator::{verify, VerifyInput};
pub use record::{Mechanism, Qualifier, Record};
pub use resolver::SpfResolver;

#[cfg(feature = "hickory")]
pub use resolver::hickory::HickoryResolver;

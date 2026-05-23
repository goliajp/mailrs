#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Module layout:
//! - [`header`]    — DKIM-Signature header parser
//! - [`canon`]     — header + body canonicalization (simple / relaxed)
//! - [`headers`]   — low-level byte-region helpers (body offset, fold-aware find, b= clear)
//! - [`crypto`]    — standalone RSA-SHA256 / Ed25519-SHA256 signature verify + DNS pubkey parse
//! - [`resolver`]  — [`DkimResolver`] trait + (optional) hickory impl
//! - [`verifier`]  — full verify() entry point
//! - [`error`]     — error / temp-fail / perm-fail types

pub mod canon;
pub mod crypto;
pub mod error;
pub mod header;
pub mod headers;
pub mod resolver;
pub mod verifier;

pub use error::{DkimError, DkimResult};
pub use header::{Algorithm, Canon, DkimHeader};
pub use resolver::DkimResolver;
pub use verifier::verify;

#[cfg(feature = "hickory")]
pub use resolver::hickory::HickoryDkimResolver;

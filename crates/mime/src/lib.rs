#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Module layout:
//! - [`content_type`] — parse `Content-Type:` header (type / subtype / params)
//! - [`decoder`]      — Content-Transfer-Encoding decoders (base64, qp, 7bit, 8bit)
//! - [`part`]         — `Part` struct + multipart tree walker

pub mod content_type;
pub mod decoder;
pub mod part;

pub use content_type::{ContentType, Disposition};
pub use decoder::TransferEncoding;
pub use part::{Part, parse};
